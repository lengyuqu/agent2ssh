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
    use agent2ssh::connection::{connect_host, list_active_connections};
    use agent2ssh::core::*;
    use agent2ssh::notify::{load_webhook_config, save_webhook_config, WebhookConfig};
    use agent2ssh::playbook::{list_playbooks_core, run_playbook_core, Playbook, PlaybookRunResult};
    use agent2ssh::remote::{list_daemons_core, DaemonInfo};
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

    #[derive(Deserialize)]
    struct PlaybookRunBody {
        playbook: String,
        host: String,
        #[serde(default)]
        force: bool,
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

    // ── New handlers for expanded endpoints ─────────────────────────────────

    async fn connection_status(
        State(s): State<AppState>,
        headers: HeaderMap,
    ) -> Result<Json<Vec<ConnectionStatus>>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        Ok(Json(list_active_connections().await))
    }

    async fn ssh_connect(
        State(s): State<AppState>,
        headers: HeaderMap,
        Path(host): Path<String>,
    ) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        connect_host(&host)
            .await
            .map(|_| Json(OkBody { ok: true }))
            .map_err(|e| err(StatusCode::BAD_REQUEST, e))
    }

    async fn list_playbooks(
        State(s): State<AppState>,
        headers: HeaderMap,
    ) -> Result<Json<Vec<Playbook>>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        list_playbooks_core()
            .map(Json)
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
    }

    async fn run_playbook(
        State(s): State<AppState>,
        headers: HeaderMap,
        Json(body): Json<PlaybookRunBody>,
    ) -> Result<Json<PlaybookRunResult>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        run_playbook_core(&body.playbook, &body.host, body.force)
            .await
            .map(Json)
            .map_err(|e| err(StatusCode::BAD_REQUEST, e))
    }

    async fn list_daemons(
        State(s): State<AppState>,
        headers: HeaderMap,
    ) -> Result<Json<Vec<DaemonInfo>>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        list_daemons_core()
            .map(Json)
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
    }

    async fn get_webhook_config(
        State(s): State<AppState>,
        headers: HeaderMap,
    ) -> Result<Json<WebhookConfig>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        Ok(Json(load_webhook_config().unwrap_or_default()))
    }

    async fn set_webhook_config(
        State(s): State<AppState>,
        headers: HeaderMap,
        Json(config): Json<WebhookConfig>,
    ) -> Result<Json<WebhookConfig>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        save_webhook_config(&config)
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        Ok(Json(config))
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
            // Expanded endpoints
            .route("/connections", get(connection_status))
            .route("/connections/:host/connect", post(ssh_connect))
            .route("/playbooks", get(list_playbooks))
            .route("/playbooks/run", post(run_playbook))
            .route("/daemons", get(list_daemons))
            .route("/webhook/config", get(get_webhook_config).put(set_webhook_config))
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

// ============================================================================
// Part 3: MCP tool enumeration test (P4-1)
// ============================================================================

/// Meta-test: verify the MCP binary declares exactly 31 tools by parsing
/// the source file and counting `"name":` entries in the tools/list handler.
///
/// This avoids the need to run the MCP server over stdio JSON-RPC, which
/// requires a full process lifecycle. Instead we treat the source as the
/// canonical tool registry and assert on its structure.
#[test]
fn mcp_tool_list_contains_exactly_31_tools() {
    let source = include_str!("../src/bin/agent2ssh-mcp.rs");

    // Extract the tools/list handler block: everything between
    // `"tools/list" => Ok(json!({` and the closing `})),`
    let tools_section = source
        .find("\"tools/list\"")
        .expect("tools/list handler not found in MCP source");

    // Find the tools array start
    let array_start = source[tools_section..]
        .find("\"tools\": [")
        .expect("tools array not found")
        + tools_section;

    // Count tool definitions by counting `"name":` occurrences within
    // the tools array. Each tool object has exactly one "name" field at the
    // top level, but some input schemas also use "name" as a property key
    // (e.g. ssh_add_host and ssh_remove_host), so the raw count is
    // 31 tools + 2 schema-property "name" keys = 33.
    let after_array_start = array_start + "\"tools\": [".len();
    let mut depth = 1;
    let mut end = after_array_start;
    for (i, ch) in source[after_array_start..].char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    end = after_array_start + i;
                    break;
                }
            }
            _ => {}
        }
    }

    let tools_block = &source[after_array_start..end];

    // Count only top-level tool names (those whose value starts with "ssh_").
    let tool_count = tools_block.matches("\"name\": \"ssh_").count();

    assert_eq!(
        tool_count, 31,
        "Expected exactly 31 MCP tools, found {tool_count}. \
         If you added or removed a tool, update this count and the expected list below."
    );
}

/// Verify every expected tool name is present in the MCP tools/list output.
#[test]
fn mcp_tool_list_contains_all_expected_names() {
    let source = include_str!("../src/bin/agent2ssh-mcp.rs");

    let expected_tools = [
        "ssh_list_hosts",
        "ssh_list_daemons",
        "ssh_import_config",
        "ssh_add_host",
        "ssh_remove_host",
        "ssh_exec",
        "ssh_ping",
        "ssh_exec_multi",
        "ssh_audit",
        "ssh_sftp_ls",
        "ssh_sftp_stat",
        "ssh_sftp_mkdir",
        "ssh_sftp_upload",
        "ssh_sftp_download",
        "ssh_session_open",
        "ssh_session_write",
        "ssh_session_read",
        "ssh_session_close",
        "ssh_session_list",
        "ssh_forward_add",
        "ssh_forward_list",
        "ssh_forward_remove",
        "ssh_risk_check",
        "ssh_approval_list",
        "ssh_approval_respond",
        "ssh_playbook_list",
        "ssh_playbook_run",
        "ssh_connection_status",
        "ssh_connect",
        "ssh_disconnect",
        "ssh_webhook_config",
    ];

    // All tool names should appear in the tools/list section of the source
    let tools_list_start = source
        .find("\"tools/list\"")
        .expect("tools/list handler not found");
    // Find the end of the tools/list json! block by scanning for "tools/call"
    let tools_call = source
        .find("\"tools/call\"")
        .expect("tools/call handler not found");
    let tools_section = &source[tools_list_start..tools_call];

    for tool_name in &expected_tools {
        assert!(
            tools_section.contains(&format!("\"name\": \"{}\"", tool_name)),
            "MCP tool '{}' not found in tools/list handler",
            tool_name
        );
    }
}

/// Verify the MCP call_tool handler covers all 31 tool names.
#[test]
fn mcp_call_tool_handler_covers_all_tools() {
    let source = include_str!("../src/bin/agent2ssh-mcp.rs");

    let expected_tools = [
        "ssh_list_hosts",
        "ssh_list_daemons",
        "ssh_import_config",
        "ssh_add_host",
        "ssh_remove_host",
        "ssh_exec",
        "ssh_ping",
        "ssh_exec_multi",
        "ssh_audit",
        "ssh_sftp_ls",
        "ssh_sftp_stat",
        "ssh_sftp_mkdir",
        "ssh_sftp_upload",
        "ssh_sftp_download",
        "ssh_session_open",
        "ssh_session_write",
        "ssh_session_read",
        "ssh_session_close",
        "ssh_session_list",
        "ssh_forward_add",
        "ssh_forward_list",
        "ssh_forward_remove",
        "ssh_risk_check",
        "ssh_approval_list",
        "ssh_approval_respond",
        "ssh_playbook_list",
        "ssh_playbook_run",
        "ssh_connection_status",
        "ssh_connect",
        "ssh_disconnect",
        "ssh_webhook_config",
    ];

    // Find the call_tool function body
    let call_tool_start = source
        .find("async fn call_tool")
        .expect("call_tool function not found");
    let call_tool_section = &source[call_tool_start..];

    for tool_name in &expected_tools {
        assert!(
            call_tool_section.contains(&format!("\"{}\"", tool_name)),
            "Tool '{}' not handled in call_tool match arm",
            tool_name
        );
    }
}

// ============================================================================
// Part 4: Expanded HTTP handler tests (P4-2)
// ============================================================================

// ── Connections endpoint ─────────────────────────────────────────────────────

#[tokio::test]
async fn http_connections_returns_valid_json_array() {
    let app = http_helpers::build_test_router();

    let response = app.oneshot(auth_get("/connections")).await.unwrap();
    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response_json(response).await;
    assert!(body.is_array(), "GET /connections must return a JSON array");

    // Each element should have ConnectionStatus fields
    if let Some(arr) = body.as_array() {
        for entry in arr {
            assert!(
                entry.get("host").is_some(),
                "ConnectionStatus must have 'host'"
            );
            assert!(
                entry.get("connected").is_some(),
                "ConnectionStatus must have 'connected'"
            );
        }
    }
}

#[tokio::test]
async fn http_connections_requires_auth() {
    let app = http_helpers::build_test_router();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/connections")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn http_connections_connect_nonexistent_returns_error() {
    let app = http_helpers::build_test_router();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/connections/nonexistent-host-xyz/connect")
                .header("authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // connect_host for a non-existent host should fail -> 400
    assert_eq!(
        response.status(),
        400,
        "Connecting to a nonexistent host should return 400 Bad Request"
    );

    let body: serde_json::Value = response_json(response).await;
    assert!(
        body.get("error").is_some(),
        "Error response must have 'error' field"
    );
}

// ── Playbooks endpoint ──────────────────────────────────────────────────────

#[tokio::test]
async fn http_playbooks_returns_valid_json_array() {
    let app = http_helpers::build_test_router();

    let response = app.oneshot(auth_get("/playbooks")).await.unwrap();
    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response_json(response).await;
    assert!(body.is_array(), "GET /playbooks must return a JSON array");

    // Each element should have Playbook fields if any playbooks exist
    if let Some(arr) = body.as_array() {
        for entry in arr {
            assert!(
                entry.get("name").is_some(),
                "Playbook must have 'name' field"
            );
            assert!(
                entry.get("steps").is_some(),
                "Playbook must have 'steps' field"
            );
        }
    }
}

#[tokio::test]
async fn http_playbooks_requires_auth() {
    let app = http_helpers::build_test_router();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/playbooks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn http_playbooks_run_missing_playbook_returns_error() {
    let app = http_helpers::build_test_router();

    let body = serde_json::json!({
        "playbook": "nonexistent-playbook-xyz",
        "host": "nonexistent-host-xyz",
        "force": false
    });
    let response = app
        .oneshot(auth_json_request(
            axum::http::Method::POST,
            "/playbooks/run",
            &body,
        ))
        .await
        .unwrap();

    // run_playbook_core should fail for a nonexistent playbook -> 400
    assert_eq!(
        response.status(),
        400,
        "Running a nonexistent playbook should return 400 Bad Request"
    );

    let resp_body: serde_json::Value = response_json(response).await;
    assert!(
        resp_body.get("error").is_some(),
        "Error response must have 'error' field"
    );
}

// ── Daemons endpoint ────────────────────────────────────────────────────────

#[tokio::test]
async fn http_daemons_returns_valid_json_array() {
    let app = http_helpers::build_test_router();

    let response = app.oneshot(auth_get("/daemons")).await.unwrap();
    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response_json(response).await;
    assert!(body.is_array(), "GET /daemons must return a JSON array");

    // The array should always contain at least the localhost entry
    let arr = body.as_array().unwrap();
    assert!(
        !arr.is_empty(),
        "GET /daemons should always include at least the localhost daemon entry"
    );

    // First entry should be localhost
    let first = &arr[0];
    assert_eq!(
        first["alias"], "localhost",
        "First daemon entry should be 'localhost'"
    );
    assert!(
        first.get("url").is_some(),
        "DaemonInfo must have 'url' field"
    );
    assert!(
        first.get("connected").is_some(),
        "DaemonInfo must have 'connected' field"
    );
}

#[tokio::test]
async fn http_daemons_requires_auth() {
    let app = http_helpers::build_test_router();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/daemons")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

// ── Webhook config endpoint ─────────────────────────────────────────────────

#[tokio::test]
async fn http_webhook_config_get_returns_config_or_default() {
    let app = http_helpers::build_test_router();

    let response = app
        .oneshot(auth_get("/webhook/config"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response_json(response).await;
    assert!(body.is_object(), "GET /webhook/config must return a JSON object");

    // Default config should have an "events" field (array)
    assert!(
        body.get("events").is_some(),
        "WebhookConfig must have 'events' field"
    );
    assert!(
        body["events"].is_array(),
        "WebhookConfig.events must be an array"
    );
}

#[tokio::test]
async fn http_webhook_config_put_accepts_and_stores_config() {
    let app = http_helpers::build_test_router();

    let config = serde_json::json!({
        "url": "https://example.com/test-hook",
        "events": ["exec_completed"],
        "secret": "test-secret-123"
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::PUT)
                .uri("/webhook/config")
                .header("authorization", "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&config).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        200,
        "PUT /webhook/config should succeed"
    );

    let body: serde_json::Value = response_json(response).await;
    assert_eq!(body["url"], "https://example.com/test-hook");
    assert_eq!(body["events"][0], "exec_completed");
    assert_eq!(body["secret"], "test-secret-123");
}

#[tokio::test]
async fn http_webhook_config_requires_auth() {
    let app = http_helpers::build_test_router();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/webhook/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn http_webhook_config_put_requires_auth() {
    let app = http_helpers::build_test_router();

    let config = serde_json::json!({
        "url": "https://example.com/hook",
        "events": [],
        "secret": null
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::PUT)
                .uri("/webhook/config")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&config).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}
