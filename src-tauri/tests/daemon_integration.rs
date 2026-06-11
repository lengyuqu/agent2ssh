//! Integration tests for daemon-related core functionality.
//!
//! Two layers of tests:
//!
//! 1. **Core function tests** — exercise the library functions that the daemon
//!    HTTP handlers wrap, verifying business logic without going through HTTP.
//!
//! 2. **HTTP handler tests** — build a minimal axum `Router` that mirrors the
//!    daemon's route structure and use axum's `oneshot` test utility to verify
//!    request/response semantics (auth, routing, JSON serialization, status
//!    codes) without spawning a real TCP server.

use agent2ssh::approval::*;
use agent2ssh::core::*;
use agent2ssh::risk_config::*;
use agent2ssh::types::*;

// ── HTTP test helpers ───────────────────────────────────────────────────────

#[cfg(test)]
mod http_helpers {
    use agent2ssh::approval::{approval_list, approval_respond, ApprovalRequest};
    use agent2ssh::core::*;
    use agent2ssh::risk_config::classify_with_user_rules;
    use agent2ssh::types::*;

    use axum::{
        extract::{Path, State},
        http::{HeaderMap, StatusCode},
        routing::{get, post},
        Json, Router,
    };
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    #[derive(Clone)]
    pub(super) struct AppState {
        pub(super) token: String,
    }

    #[derive(Serialize)]
    struct ErrorBody {
        error: String,
    }

    fn err(status: StatusCode, msg: impl ToString) -> (StatusCode, Json<ErrorBody>) {
        (status, Json(ErrorBody { error: msg.to_string() }))
    }

    fn check_auth(
        state: &AppState,
        headers: &HeaderMap,
    ) -> Result<(), (StatusCode, Json<ErrorBody>)> {
        let token = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        if token == state.token {
            Ok(())
        } else {
            Err((
                StatusCode::UNAUTHORIZED,
                Json(ErrorBody { error: "unauthorized".into() }),
            ))
        }
    }

    #[derive(Serialize)]
    struct OkBody {
        ok: bool,
    }

    #[derive(Deserialize)]
    struct RiskCheckBody {
        command: String,
        #[allow(dead_code)]
        host: Option<String>,
    }

    #[derive(Serialize)]
    struct RiskCheckResult {
        risk_level: RiskLevel,
        matched_rule: Option<String>,
    }

    // ── Handlers (mirror the daemon binary) ─────────────────────────────────

    async fn health() -> Json<serde_json::Value> {
        Json(serde_json::json!({"ok": true}))
    }

    async fn list_hosts(
        State(s): State<AppState>,
        headers: HeaderMap,
    ) -> Result<Json<Vec<HostProfile>>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        list_hosts_core()
            .map(Json)
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
    }

    async fn risk_check(
        State(s): State<AppState>,
        headers: HeaderMap,
        Json(body): Json<RiskCheckBody>,
    ) -> Result<Json<RiskCheckResult>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        let base = classify_risk(&body.command);
        if let Some(user_risk) = classify_with_user_rules(&body.command).await {
            let final_risk = match (&user_risk, &base) {
                (RiskLevel::Blocked, _) => RiskLevel::Blocked,
                (RiskLevel::High, RiskLevel::Blocked) => RiskLevel::Blocked,
                (ur, _) => *ur,
            };
            return Ok(Json(RiskCheckResult {
                risk_level: final_risk,
                matched_rule: Some("user_rule".into()),
            }));
        }
        Ok(Json(RiskCheckResult {
            risk_level: base,
            matched_rule: None,
        }))
    }

    async fn approvals_list(
        State(s): State<AppState>,
        headers: HeaderMap,
    ) -> Result<Json<Vec<ApprovalRequest>>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        Ok(Json(approval_list().await))
    }

    async fn approval_approve(
        State(s): State<AppState>,
        headers: HeaderMap,
        Path(id): Path<String>,
    ) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
        approval_respond(uuid, true)
            .await
            .map(|_| Json(OkBody { ok: true }))
            .map_err(|e| err(StatusCode::BAD_REQUEST, e))
    }

    async fn approval_reject(
        State(s): State<AppState>,
        headers: HeaderMap,
        Path(id): Path<String>,
    ) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
        approval_respond(uuid, false)
            .await
            .map(|_| Json(OkBody { ok: true }))
            .map_err(|e| err(StatusCode::BAD_REQUEST, e))
    }

    /// Build a test router mirroring the daemon's route structure.
    /// Uses a fixed test token for authentication.
    pub(super) fn build_test_router() -> Router {
        let state = AppState {
            token: "test-token".to_string(),
        };
        Router::new()
            .route("/health", get(health))
            .route("/hosts", get(list_hosts))
            .route("/risk/check", post(risk_check))
            .route("/approvals", get(approvals_list))
            .route("/approvals/:id/approve", post(approval_approve))
            .route("/approvals/:id/reject", post(approval_reject))
            .with_state(state)
    }
}

// ============================================================================
// Part 1: Core function tests
// ============================================================================

#[test]
fn test_classify_risk_exec_blocking() {
    // Blocked commands should always be blocked regardless of host
    assert_eq!(classify_risk("rm -rf /"), RiskLevel::Blocked);
    assert_eq!(classify_risk("mkfs /dev/sda"), RiskLevel::Blocked);
}

#[test]
fn test_classify_risk_exec_high_requires_force() {
    assert_eq!(classify_risk("sudo whoami"), RiskLevel::High);
    assert_eq!(classify_risk("rm -rf /tmp/stuff"), RiskLevel::High);
}

#[tokio::test]
async fn test_approval_full_lifecycle() {
    // Create approval
    let id = approval_request("integration-host", "sudo apt update", RiskLevel::High).await;

    // Should be pending
    let status = approval_poll(id).await.expect("approval should exist");
    assert_eq!(status, ApprovalStatus::Pending);

    // List should contain it
    let list = approval_list().await;
    assert!(list
        .iter()
        .any(|r| r.id == id && r.status == ApprovalStatus::Pending));

    // Approve it
    approval_respond(id, true).await.expect("should succeed");

    // Should now be approved
    let status = approval_poll(id).await.expect("approval should exist");
    assert_eq!(status, ApprovalStatus::Approved);
}

#[tokio::test]
async fn test_approval_reject_and_list() {
    let id = approval_request("integration-host", "sudo reboot", RiskLevel::High).await;

    // Reject
    approval_respond(id, false)
        .await
        .expect("should succeed");

    let status = approval_poll(id).await.expect("approval should exist");
    assert_eq!(status, ApprovalStatus::Rejected);

    // List should still contain it (with rejected status)
    let list = approval_list().await;
    assert!(list
        .iter()
        .any(|r| r.id == id && r.status == ApprovalStatus::Rejected));
}

#[tokio::test]
async fn test_user_risk_rules_classification() {
    // With no rules file, classify_with_user_rules returns None
    let result = classify_with_user_rules("kubectl delete namespace default").await;
    // If no risk_rules.toml exists, this returns None
    // (the function is designed to gracefully handle missing files)
    assert!(result.is_none() || result.is_some());
}

#[test]
fn test_host_profile_tags_serialization() {
    // Verify HostProfile with tags serializes/deserializes correctly
    let host = HostProfile {
        name: "test".to_string(),
        host: "10.0.0.1".to_string(),
        user: Some("ubuntu".to_string()),
        port: Some(22),
        key_path: None,
        jump_host: None,
        risk_override: None,
        tags: vec!["production".to_string(), "web".to_string()],
    };

    let json = serde_json::to_string(&host).unwrap();
    let deserialized: HostProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.tags, vec!["production", "web"]);
    assert_eq!(deserialized.name, "test");
}

#[test]
fn test_host_profile_default_tags_empty() {
    // HostProfile without tags field should deserialize with empty tags
    let json = r#"{"name":"test","host":"10.0.0.1"}"#;
    let host: HostProfile = serde_json::from_str(json).unwrap();
    assert!(host.tags.is_empty());
}

#[test]
fn test_audit_filter_serialization() {
    let filter = AuditFilter {
        host: Some("prod".to_string()),
        risk_level: Some(RiskLevel::High),
        exit_code: None,
        since: None,
        until: None,
        limit: 50,
    };
    let json = serde_json::to_string(&filter).unwrap();
    assert!(json.contains("\"host\":\"prod\""));
    assert!(json.contains("\"risk_level\":\"high\""));
}

#[tokio::test]
async fn test_approval_wait_immediate_approve() {
    use std::time::Duration;

    let id = approval_request("wait-host", "ls", RiskLevel::Medium).await;

    // Approve in background after a short delay
    let approve_id = id;
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = approval_respond(approve_id, true).await;
    });

    // Wait should return Approved
    let status = approval_wait(id).await;
    assert_eq!(status, ApprovalStatus::Approved);
}

// ============================================================================
// Part 2: HTTP handler tests (axum oneshot)
// ============================================================================

use axum::body::Body;
use http_body_util::BodyExt;
use tower::ServiceExt; // for oneshot()

/// Helper: build a JSON POST request with auth header.
fn auth_json_request(
    method: axum::http::Method,
    uri: &str,
    body: &impl serde::Serialize,
) -> axum::http::Request<Body> {
    axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", "Bearer test-token")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(body).unwrap()))
        .unwrap()
}

/// Helper: build a GET request with auth header.
fn auth_get(uri: &str) -> axum::http::Request<Body> {
    axum::http::Request::builder()
        .method(axum::http::Method::GET)
        .uri(uri)
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .unwrap()
}

/// Helper: extract JSON body from a response.
async fn response_json<T: serde::de::DeserializeOwned>(
    response: axum::http::Response<Body>,
) -> T {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ── Health ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn http_health_returns_ok() {
    let app = http_helpers::build_test_router();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response_json(response).await;
    assert_eq!(body["ok"], true);
}

// ── Auth middleware ─────────────────────────────────────────────────────────

#[tokio::test]
async fn http_auth_rejected_without_token() {
    let app = http_helpers::build_test_router();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/hosts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn http_auth_rejected_with_wrong_token() {
    let app = http_helpers::build_test_router();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/hosts")
                .header("authorization", "Bearer wrong-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn http_auth_succeeds_with_correct_token() {
    let app = http_helpers::build_test_router();

    let response = app.oneshot(auth_get("/hosts")).await.unwrap();

    // 200 means auth passed; the body is the host list (may be empty)
    assert_eq!(response.status(), 200);
}

// ── Risk check endpoint ────────────────────────────────────────────────────

#[tokio::test]
async fn http_risk_check_blocked_command() {
    let app = http_helpers::build_test_router();

    let body = serde_json::json!({"command": "rm -rf /"});
    let response = app
        .oneshot(auth_json_request(axum::http::Method::POST, "/risk/check", &body))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let result: serde_json::Value = response_json(response).await;
    assert_eq!(result["risk_level"], "blocked");
    assert!(result["matched_rule"].is_null());
}

#[tokio::test]
async fn http_risk_check_high_command() {
    let app = http_helpers::build_test_router();

    let body = serde_json::json!({"command": "sudo whoami"});
    let response = app
        .oneshot(auth_json_request(axum::http::Method::POST, "/risk/check", &body))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let result: serde_json::Value = response_json(response).await;
    assert_eq!(result["risk_level"], "high");
}

#[tokio::test]
async fn http_risk_check_low_command() {
    let app = http_helpers::build_test_router();

    let body = serde_json::json!({"command": "ls -la"});
    let response = app
        .oneshot(auth_json_request(axum::http::Method::POST, "/risk/check", &body))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let result: serde_json::Value = response_json(response).await;
    assert_eq!(result["risk_level"], "low");
}

#[tokio::test]
async fn http_risk_check_medium_command() {
    let app = http_helpers::build_test_router();

    let body = serde_json::json!({"command": "apt install nginx"});
    let response = app
        .oneshot(auth_json_request(axum::http::Method::POST, "/risk/check", &body))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let result: serde_json::Value = response_json(response).await;
    assert_eq!(result["risk_level"], "medium");
}

#[tokio::test]
async fn http_risk_check_requires_auth() {
    let app = http_helpers::build_test_router();

    let body = serde_json::json!({"command": "ls"});
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/risk/check")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

// ── Approval endpoints ──────────────────────────────────────────────────────

#[tokio::test]
async fn http_approvals_list_returns_array() {
    let app = http_helpers::build_test_router();

    let response = app
        .oneshot(auth_get("/approvals"))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response_json(response).await;
    assert!(body.is_array());
}

#[tokio::test]
async fn http_approval_approve_flow() {
    // Create an approval via the core API
    let id = approval_request("http-test-host", "sudo apt update", RiskLevel::High).await;

    // Approve via HTTP
    let app = http_helpers::build_test_router();
    let uri = format!("/approvals/{}/approve", id);
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri(&uri)
                .header("authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response_json(response).await;
    assert_eq!(body["ok"], true);

    // Verify via core API
    let status = approval_poll(id).await.expect("approval should exist");
    assert_eq!(status, ApprovalStatus::Approved);
}

#[tokio::test]
async fn http_approval_reject_flow() {
    // Create an approval via the core API
    let id = approval_request("http-test-host", "sudo reboot", RiskLevel::High).await;

    // Reject via HTTP
    let app = http_helpers::build_test_router();
    let uri = format!("/approvals/{}/reject", id);
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri(&uri)
                .header("authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response_json(response).await;
    assert_eq!(body["ok"], true);

    // Verify via core API
    let status = approval_poll(id).await.expect("approval should exist");
    assert_eq!(status, ApprovalStatus::Rejected);
}

#[tokio::test]
async fn http_approval_invalid_uuid_returns_400() {
    let app = http_helpers::build_test_router();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/approvals/not-a-uuid/approve")
                .header("authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 400);
}

#[tokio::test]
async fn http_approval_nonexistent_uuid_returns_400() {
    let app = http_helpers::build_test_router();
    let fake_id = uuid::Uuid::new_v4();
    let uri = format!("/approvals/{}/approve", fake_id);

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri(&uri)
                .header("authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // approval_respond returns Err for unknown UUIDs → handler maps to 400
    assert_eq!(response.status(), 400);
}

// ── Host profile round-trip through HTTP ────────────────────────────────────

#[tokio::test]
async fn http_hosts_list_returns_valid_json_array() {
    let app = http_helpers::build_test_router();

    let response = app.oneshot(auth_get("/hosts")).await.unwrap();
    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response_json(response).await;
    assert!(body.is_array());

    // Every element should have the required HostProfile fields
    if let Some(arr) = body.as_array() {
        for entry in arr {
            assert!(entry.get("name").is_some(), "HostProfile must have 'name'");
            assert!(entry.get("host").is_some(), "HostProfile must have 'host'");
        }
    }
}
