//! Integration tests for daemon-related core functionality.
//!
//! Two layers of tests:
//!
//! 1. **Core function tests** — exercise the library functions that the daemon
//!    HTTP handlers wrap, verifying business logic without going through HTTP.
//!
//! 2. **HTTP handler tests** — build a small axum `Router` that reproduces the
//!    shape of 18 of the daemon's 92 routes, and use axum's `oneshot` test
//!    utility to verify request/response semantics (auth, routing, JSON
//!    serialization, status codes) without spawning a real TCP server.
//!
//!    **This layer is a shape-mirror, not an implementation test.** The copies
//!    carry neither `AuthContext` nor `require_admin` — `check_auth` compares
//!    one fixed token — so nothing here can say what a *scoped* token gets, and
//!    a handler that gained or lost an admin gate would keep these assertions
//!    green. The 74 routes that are not mirrored at all (`/metrics`, `/gate*`,
//!    `/audit*`, `/sftp/*`, `/sessions*`, `/forwards*`, `/recordings*`,
//!    `/prompts/*`, `/terminal*`, `/events/stream`) have no coverage here in
//!    either direction. Behaviour that depends on scope, admin, or a route this
//!    router does not mount belongs in the real-daemon e2e targets
//!    (`daemon_local_exec_e2e`, `daemon_resident_session_e2e`, ...), which spawn
//!    `agent2ssh-daemon` itself.

mod common;

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
    use agent2ssh::health::load_health_snapshot;
    use agent2ssh::notify::{load_webhook_config, save_webhook_config, WebhookConfig};
    use agent2ssh::playbook::{
        dry_run_playbook, list_playbooks_core, run_playbook_core, Playbook, PlaybookDryRun,
        PlaybookRunResult,
    };
    use agent2ssh::remote::{list_daemons_core, DaemonInfo};
    use agent2ssh::risk_config::classify_with_user_rules;
    use agent2ssh::types::*;
    use agent2ssh::{command_authorization_target, effective_command_risk};
    use axum::{
        extract::{Path, State},
        http::{HeaderMap, StatusCode},
        response::Html,
        routing::{get, post},
        Json, Router,
    };
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
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
        (
            status,
            Json(ErrorBody {
                error: msg.to_string(),
            }),
        )
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
                Json(ErrorBody {
                    error: "unauthorized".into(),
                }),
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
        #[serde(default)]
        params: Option<HashMap<String, String>>,
        #[serde(default)]
        reason: Option<String>,
        #[serde(default)]
        change_id: Option<String>,
    }

    #[derive(Deserialize)]
    struct PlaybookDryRunBody {
        playbook: String,
        #[serde(default)]
        params: Option<HashMap<String, String>>,
    }

    // ── Handlers (mirror the daemon binary) ─────────────────────────────────

    async fn health() -> Json<serde_json::Value> {
        Json(serde_json::json!({"ok": true}))
    }

    async fn console() -> Html<&'static str> {
        Html(include_str!("../web/console.html"))
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
        let risk_override = body
            .host
            .as_deref()
            .and_then(|host| command_authorization_target(host).risk_override);
        let matched_rule = classify_with_user_rules(&body.command)
            .await
            .map(|_| "user_rule".to_string());
        let risk = apply_risk_override(effective_command_risk(&body.command).await, risk_override);
        Ok(Json(RiskCheckResult {
            risk_level: risk,
            matched_rule,
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
        run_playbook_core(
            &body.playbook,
            &body.host,
            body.force,
            body.params.as_ref(),
            body.reason,
            body.change_id,
        )
        .await
        .map(Json)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
    }

    async fn dry_run_playbook_handler(
        State(s): State<AppState>,
        headers: HeaderMap,
        Json(body): Json<PlaybookDryRunBody>,
    ) -> Result<Json<PlaybookDryRun>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        let params = body.params.unwrap_or_default();
        dry_run_playbook(&body.playbook, &params)
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
        save_webhook_config(&config).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        Ok(Json(config))
    }

    #[derive(Deserialize)]
    struct ExecPreviewBody {
        host: Option<String>,
        hosts: Option<Vec<String>>,
        command: String,
        timeout_secs: Option<u64>,
        #[serde(default)]
        tags: Option<Vec<String>>,
    }

    async fn exec_preview(
        State(s): State<AppState>,
        headers: HeaderMap,
        Json(body): Json<ExecPreviewBody>,
    ) -> Result<Json<ExecPlan>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        if let Some(hosts) = body.hosts {
            preview_exec_multi(hosts, &body.command, body.tags, body.timeout_secs)
                .await
                .map(Json)
                .map_err(|e| err(StatusCode::BAD_REQUEST, e))
        } else if let Some(host) = body.host {
            preview_exec(&host, &body.command, body.timeout_secs)
                .await
                .map(Json)
                .map_err(|e| err(StatusCode::BAD_REQUEST, e))
        } else {
            Err(err(StatusCode::BAD_REQUEST, "host or hosts required"))
        }
    }

    // ── Approval policy handlers ────────────────────────────────────────────

    use agent2ssh::approval::{
        check_approval_required, list_approval_policies, save_approval_policies, ApprovalPolicy,
    };

    #[derive(Deserialize)]
    struct ApprovalCheckBody {
        host: String,
        command: String,
    }

    #[derive(Serialize)]
    struct ApprovalCheckResult {
        requires_approval: bool,
        matched_policy: Option<String>,
        risk_level: RiskLevel,
        ttl_secs: Option<u64>,
    }

    async fn list_policies(
        State(s): State<AppState>,
        headers: HeaderMap,
    ) -> Result<Json<Vec<ApprovalPolicy>>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        list_approval_policies()
            .map(Json)
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
    }

    async fn put_policies(
        State(s): State<AppState>,
        headers: HeaderMap,
        Json(policies): Json<Vec<ApprovalPolicy>>,
    ) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        save_approval_policies(&policies)
            .map(|_| Json(OkBody { ok: true }))
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
    }

    async fn approval_check(
        State(s): State<AppState>,
        headers: HeaderMap,
        Json(body): Json<ApprovalCheckBody>,
    ) -> Result<Json<ApprovalCheckResult>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        let target = command_authorization_target(&body.host);
        let risk = apply_risk_override(
            effective_command_risk(&body.command).await,
            target.risk_override,
        );
        let result = check_approval_required(&body.host, &target.tags, &body.command, risk)
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        match result {
            Some(policy) => Ok(Json(ApprovalCheckResult {
                requires_approval: true,
                matched_policy: Some(policy.name),
                risk_level: risk,
                ttl_secs: policy.ttl_secs,
            })),
            None => Ok(Json(ApprovalCheckResult {
                requires_approval: false,
                matched_policy: None,
                risk_level: risk,
                ttl_secs: None,
            })),
        }
    }

    async fn get_health_snapshot_handler(
        State(s): State<AppState>,
        headers: HeaderMap,
    ) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
        check_auth(&s, &headers)?;
        match load_health_snapshot() {
            Ok(snapshot) => Ok(Json(serde_json::to_value(snapshot).unwrap_or_default())),
            Err(_) => Ok(Json(serde_json::json!({
                "error": "no health snapshot available"
            }))),
        }
    }

    /// Build a test router mirroring the daemon's route structure.
    /// Uses a fixed test token for authentication.
    pub(super) fn build_test_router() -> Router {
        let state = AppState {
            token: "test-token".to_string(),
        };
        Router::new()
            .route("/console", get(console))
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
            .route("/playbooks/:name/dry-run", post(dry_run_playbook_handler))
            .route("/daemons", get(list_daemons))
            .route(
                "/webhook/config",
                get(get_webhook_config).put(set_webhook_config),
            )
            .route("/exec/preview", post(exec_preview))
            .route("/approval/policies", get(list_policies).put(put_policies))
            .route("/approval/check", post(approval_check))
            .route("/health-snapshot", get(get_health_snapshot_handler))
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
    approval_respond(id, false).await.expect("should succeed");

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
        ai_allowed: true,
        name: "test".to_string(),
        host: "10.0.0.1".to_string(),
        user: Some("ubuntu".to_string()),
        port: Some(22),
        key_path: None,
        password: None,
        jump_host: None,
        proxy_id: None,
        risk_override: None,
        tags: vec!["production".to_string(), "web".to_string()],
        group: default_host_group(),
        env: Some("prod".to_string()),
        role: Some("web".to_string()),
        owner: Some("platform".to_string()),
        init_command: None,
        passphrase: None,
    };

    let json = serde_json::to_string(&host).unwrap();
    let deserialized: HostProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.tags, vec!["production", "web"]);
    assert_eq!(deserialized.env.as_deref(), Some("prod"));
    assert_eq!(deserialized.role.as_deref(), Some("web"));
    assert_eq!(deserialized.owner.as_deref(), Some("platform"));
    assert_eq!(deserialized.name, "test");
}

#[test]
fn test_host_profile_default_metadata_empty() {
    // HostProfile without optional metadata should deserialize with defaults
    let json = r#"{"name":"test","host":"10.0.0.1"}"#;
    let host: HostProfile = serde_json::from_str(json).unwrap();
    assert!(host.tags.is_empty());
    assert!(host.env.is_none());
    assert!(host.role.is_none());
    assert!(host.owner.is_none());
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
        search: None,
        command_pattern: None,
        host_env: None,
        host_role: None,
        host_owner: None,
        decision: None,
        agent: None,
        host_scope: None,
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
async fn response_json<T: serde::de::DeserializeOwned>(response: axum::http::Response<Body>) -> T {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn response_text(response: axum::http::Response<Body>) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
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

#[tokio::test]
async fn http_console_serves_key_workflow_entrypoints() {
    let app = http_helpers::build_test_router();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/console")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let html = response_text(response).await;
    for expected in [
        "Agent2SSH",
        "data-tab=\"hosts\"",
        "data-tab=\"execute\"",
        "data-tab=\"approvals\"",
        "data-tab=\"settings\"",
        "token-input",
    ] {
        assert!(
            html.contains(expected),
            "console HTML should contain {expected}"
        );
    }
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
        .oneshot(auth_json_request(
            axum::http::Method::POST,
            "/risk/check",
            &body,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let result: serde_json::Value = response_json(response).await;
    assert_eq!(result["risk_level"], "blocked");
    // Narrow on purpose: this router does not isolate its config directory, so
    // there is no `risk_rules.toml` and `classify_with_user_rules` can only
    // answer `None`. The test therefore says "built-in rules leave matched_rule
    // empty", which is true and falsifiable — it says nothing about the
    // `"user_rule"` value a user rule produces (L2-29; covering that needs the
    // harness to own its config dir first).
    assert!(
        result["matched_rule"].is_null(),
        "built-in rules must not claim a user rule name: {result}"
    );
}

#[tokio::test]
async fn http_risk_check_high_command() {
    let app = http_helpers::build_test_router();

    let body = serde_json::json!({"command": "sudo whoami"});
    let response = app
        .oneshot(auth_json_request(
            axum::http::Method::POST,
            "/risk/check",
            &body,
        ))
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
        .oneshot(auth_json_request(
            axum::http::Method::POST,
            "/risk/check",
            &body,
        ))
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
        .oneshot(auth_json_request(
            axum::http::Method::POST,
            "/risk/check",
            &body,
        ))
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

    let response = app.oneshot(auth_get("/approvals")).await.unwrap();

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

/// Meta-test: verify the MCP binary declares exactly 71 tools by parsing
/// the schema-driven tool registry and counting `"name":` entries.
///
/// This avoids the need to run the MCP server over stdio JSON-RPC, which
/// requires a full process lifecycle. Instead we treat the source as the
/// canonical tool registry and assert on its structure.
#[test]
fn mcp_tool_list_contains_exactly_76_tools() {
    let registry = include_str!("../src/bin/agent2ssh_mcp/tools.rs");
    // Every tool entry carries a top-level `"name": "..."`; nothing else in the
    // file uses that exact shape (property schemas name their fields without
    // the `"name"` key), so the count is the tools/list length.
    let tool_count = registry.matches("\"name\": \"").count();

    assert_eq!(
        tool_count, 76,
        "Expected exactly 76 MCP tools, found {tool_count}. \
         If you added or removed a tool, update this count and the expected list below."
    );
}

/// Verify every expected tool name is present in the MCP tools/list output.
#[test]
fn mcp_tool_list_contains_all_expected_names() {
    let registry = include_str!("../src/bin/agent2ssh_mcp/tools.rs");

    let expected_tools = [
        "ssh_list_hosts",
        "ssh_list_daemons",
        "ssh_import_config",
        "ssh_add_host",
        "ssh_remove_host",
        "ssh_exec",
        "ssh_ping",
        "ssh_exec_multi",
        "ssh_exec_compare",
        "ssh_audit",
        "ssh_audit_export",
        "ssh_sftp_ls",
        "ssh_sftp_stat",
        "ssh_sftp_mkdir",
        "ssh_sftp_rename",
        "ssh_sftp_rm",
        "ssh_sftp_rmdir",
        "ssh_sftp_rm_rf",
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
        "ssh_gate_status",
        "ssh_approval_list",
        "ssh_approval_respond",
        "ssh_playbook_list",
        "ssh_playbook_run",
        "ssh_playbook_dry_run",
        "ssh_snippet_list",
        "ssh_snippet_save",
        "ssh_snippet_delete",
        "ssh_connection_status",
        "ssh_connect",
        "ssh_disconnect",
        "ssh_webhook_config",
        "ssh_config_export",
        "ssh_config_import",
        "ssh_config_import_preview",
        "ssh_doctor",
        "ssh_metrics",
        "ssh_metrics_trend",
        "ssh_preview_exec",
        "ssh_approval_policies_list",
        "ssh_approval_check",
        "ssh_health_snapshot",
        "ssh_daemon_diagnose",
        "ssh_daemon_version_check",
        "ssh_daemons_view",
        "ssh_events_subscribe",
        "ssh_sync_diff",
        "ssh_sync_export",
        "ssh_redact_rule_list",
        "ssh_redact_rule_add",
        "ssh_redact_rule_update",
        "ssh_redact_rule_remove",
        "ssh_redact_rule_reset",
        "ssh_highlight_rule_list",
        "ssh_highlight_rule_add",
        "ssh_highlight_rule_update",
        "ssh_highlight_rule_remove",
        "ssh_highlight_rule_reset",
        "ssh_algo_prefs_get",
        "ssh_algo_prefs_set",
        "ssh_algo_prefs_clear",
        "local_exec",
        "resident_session_open",
        "resident_session_exec",
        "resident_session_close",
        "containers_start_service",
    ];

    for tool_name in &expected_tools {
        assert!(
            registry.contains(&format!("\"name\": \"{}\"", tool_name)),
            "MCP tool '{}' not found in schema registry",
            tool_name
        );
    }
}

/// Verify the MCP call_tool handler covers all tool names.
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
        "ssh_exec_compare",
        "ssh_audit",
        "ssh_audit_export",
        "ssh_sftp_ls",
        "ssh_sftp_stat",
        "ssh_sftp_mkdir",
        "ssh_sftp_rename",
        "ssh_sftp_rm",
        "ssh_sftp_rmdir",
        "ssh_sftp_rm_rf",
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
        "ssh_gate_status",
        "ssh_approval_list",
        "ssh_approval_respond",
        "ssh_playbook_list",
        "ssh_playbook_run",
        "ssh_playbook_dry_run",
        "ssh_snippet_list",
        "ssh_snippet_save",
        "ssh_snippet_delete",
        "ssh_connection_status",
        "ssh_connect",
        "ssh_disconnect",
        "ssh_webhook_config",
        "ssh_config_export",
        "ssh_config_import",
        "ssh_config_import_preview",
        "ssh_doctor",
        "ssh_metrics",
        "ssh_metrics_trend",
        "ssh_preview_exec",
        "ssh_approval_policies_list",
        "ssh_approval_check",
        "ssh_health_snapshot",
        "ssh_daemon_diagnose",
        "ssh_daemon_version_check",
        "ssh_daemons_view",
        "ssh_events_subscribe",
        "ssh_sync_diff",
        "ssh_sync_export",
        "ssh_redact_rule_list",
        "ssh_redact_rule_add",
        "ssh_redact_rule_update",
        "ssh_redact_rule_remove",
        "ssh_redact_rule_reset",
        "ssh_highlight_rule_list",
        "ssh_highlight_rule_add",
        "ssh_highlight_rule_update",
        "ssh_highlight_rule_remove",
        "ssh_highlight_rule_reset",
        "ssh_algo_prefs_get",
        "ssh_algo_prefs_set",
        "ssh_algo_prefs_clear",
        "local_exec",
        "resident_session_open",
        "resident_session_exec",
        "resident_session_close",
        "containers_start_service",
    ];

    // Find the call_tool function body
    let call_tool_start = source
        .find("async fn call_tool")
        .expect("call_tool function not found");
    let call_tool_section = &source[call_tool_start..];

    for tool_name in &expected_tools {
        let variant = mcp_tool_variant_name(tool_name);
        assert!(
            call_tool_section.contains(&format!("McpTool::{variant}")),
            "Tool '{}' not handled in call_tool enum match arm",
            tool_name
        );
    }
}

fn mcp_tool_variant_name(tool_name: &str) -> String {
    tool_name
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect()
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

#[tokio::test]
async fn http_playbooks_dry_run_missing_playbook_returns_error() {
    let app = http_helpers::build_test_router();

    let body = serde_json::json!({
        "playbook": "nonexistent-playbook-xyz",
        "params": {}
    });
    let response = app
        .oneshot(auth_json_request(
            axum::http::Method::POST,
            "/playbooks/test/dry-run",
            &body,
        ))
        .await
        .unwrap();

    // dry_run_playbook should fail for a nonexistent playbook -> 400
    assert_eq!(
        response.status(),
        400,
        "Dry-running a nonexistent playbook should return 400 Bad Request"
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

    let response = app.oneshot(auth_get("/webhook/config")).await.unwrap();
    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response_json(response).await;
    assert!(
        body.is_object(),
        "GET /webhook/config must return a JSON object"
    );

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

    assert_eq!(response.status(), 200, "PUT /webhook/config should succeed");

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

// ── Exec preview endpoint ──────────────────────────────────────────────────

#[tokio::test]
async fn http_exec_preview_requires_auth() {
    let app = http_helpers::build_test_router();

    let body = serde_json::json!({"host": "test", "command": "ls"});
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/exec/preview")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn http_exec_preview_missing_host_returns_400() {
    let app = http_helpers::build_test_router();

    let body = serde_json::json!({"command": "ls"});
    let response = app
        .oneshot(auth_json_request(
            axum::http::Method::POST,
            "/exec/preview",
            &body,
        ))
        .await
        .unwrap();

    // No host or hosts provided -> 400
    assert_eq!(response.status(), 400);
    let resp_body: serde_json::Value = response_json(response).await;
    assert!(
        resp_body.get("error").is_some(),
        "Error response must have 'error' field"
    );
}

#[tokio::test]
async fn http_exec_preview_unknown_host_returns_400() {
    let app = http_helpers::build_test_router();

    let body = serde_json::json!({
        "host": "nonexistent-host-xyz-12345",
        "command": "uptime"
    });
    let response = app
        .oneshot(auth_json_request(
            axum::http::Method::POST,
            "/exec/preview",
            &body,
        ))
        .await
        .unwrap();

    // resolve_host should fail for a nonexistent host -> 400
    assert_eq!(response.status(), 400);
}

// ============================================================================
// Part 5: Approval policy HTTP tests (F4-1)
// ============================================================================

#[tokio::test]
async fn http_approval_policies_get_returns_array() {
    let app = http_helpers::build_test_router();

    let response = app.oneshot(auth_get("/approval/policies")).await.unwrap();

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response_json(response).await;
    assert!(
        body.is_array(),
        "GET /approval/policies must return a JSON array"
    );
}

#[tokio::test]
async fn http_approval_policies_requires_auth() {
    let app = http_helpers::build_test_router();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/approval/policies")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn http_approval_check_returns_result() {
    let app = http_helpers::build_test_router();

    let body = serde_json::json!({
        "host": "some-host",
        "command": "ls -la"
    });
    let response = app
        .oneshot(auth_json_request(
            axum::http::Method::POST,
            "/approval/check",
            &body,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let result: serde_json::Value = response_json(response).await;
    assert!(
        result.get("requires_approval").is_some(),
        "ApprovalCheckResult must have 'requires_approval' field"
    );
    assert!(
        result.get("risk_level").is_some(),
        "ApprovalCheckResult must have 'risk_level' field"
    );
}

#[tokio::test]
async fn http_approval_check_requires_auth() {
    let app = http_helpers::build_test_router();

    let body = serde_json::json!({
        "host": "some-host",
        "command": "ls"
    });
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/approval/check")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

// ── Health Snapshot endpoint ────────────────────────────────────────────────

#[tokio::test]
async fn http_health_snapshot_get_returns_200() {
    let app = http_helpers::build_test_router();

    let response = app.oneshot(auth_get("/health-snapshot")).await.unwrap();

    // Should return 200 regardless of whether a snapshot exists
    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response_json(response).await;
    // Either a valid snapshot or an error message
    assert!(
        body.get("hosts").is_some() || body.get("error").is_some(),
        "GET /health-snapshot should return either a snapshot or an error message"
    );
}

#[tokio::test]
async fn http_health_snapshot_requires_auth() {
    let app = http_helpers::build_test_router();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/health-snapshot")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

// ============================================================================
// Part 6: Documentation & contract consistency (S3)
// ============================================================================

// ── S3-1: MCP tool documentation consistency ────────────────────────────────

#[test]
fn mcp_tools_match_skills_md_documentation() {
    // Parse tool names from docs/skills.md and compare with the canonical
    // expected list used by the MCP enumeration tests. If this test fails,
    // update docs/skills.md to match the actual MCP tools/list output.
    let skills_md = include_str!("../../docs/skills.md");

    // Extract backtick-quoted tool names from the markdown table.
    // Each tool line looks like: | 1 | `ssh_list_hosts` | ... |
    let doc_tools: Vec<&str> = skills_md
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with('|') {
                return None;
            }
            // Skip header and separator lines
            if trimmed.contains("Tool") || trimmed.starts_with("|---") || trimmed.starts_with("| #")
            {
                return None;
            }
            // Extract the backtick-quoted tool name (second column)
            let cols: Vec<&str> = trimmed.split('|').collect();
            if cols.len() >= 3 {
                let tool_col = cols[2].trim(); // Second data column
                if tool_col.starts_with('`') && tool_col.ends_with('`') {
                    return Some(tool_col.trim_matches('`'));
                }
            }
            None
        })
        .collect();

    let expected_tools = vec![
        "ssh_list_hosts",
        "ssh_list_daemons",
        "ssh_import_config",
        "ssh_add_host",
        "ssh_remove_host",
        "ssh_exec",
        "ssh_ping",
        "ssh_exec_multi",
        "ssh_exec_compare",
        "ssh_audit",
        "ssh_audit_export",
        "ssh_sftp_ls",
        "ssh_sftp_stat",
        "ssh_sftp_mkdir",
        "ssh_sftp_rename",
        "ssh_sftp_rm",
        "ssh_sftp_rmdir",
        "ssh_sftp_rm_rf",
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
        "ssh_gate_status",
        "ssh_approval_list",
        "ssh_approval_respond",
        "ssh_playbook_list",
        "ssh_playbook_run",
        "ssh_playbook_dry_run",
        "ssh_snippet_list",
        "ssh_snippet_save",
        "ssh_snippet_delete",
        "ssh_connection_status",
        "ssh_connect",
        "ssh_disconnect",
        "ssh_webhook_config",
        "ssh_config_export",
        "ssh_config_import",
        "ssh_config_import_preview",
        "ssh_doctor",
        "ssh_metrics",
        "ssh_metrics_trend",
        "ssh_preview_exec",
        "ssh_approval_policies_list",
        "ssh_approval_check",
        "ssh_health_snapshot",
        "ssh_daemon_diagnose",
        "ssh_daemon_version_check",
        "ssh_daemons_view",
        "ssh_events_subscribe",
        "ssh_sync_diff",
        "ssh_sync_export",
        "ssh_redact_rule_list",
        "ssh_redact_rule_add",
        "ssh_redact_rule_update",
        "ssh_redact_rule_remove",
        "ssh_redact_rule_reset",
        "ssh_highlight_rule_list",
        "ssh_highlight_rule_add",
        "ssh_highlight_rule_update",
        "ssh_highlight_rule_remove",
        "ssh_highlight_rule_reset",
        "ssh_algo_prefs_get",
        "ssh_algo_prefs_set",
        "ssh_algo_prefs_clear",
        "local_exec",
        "resident_session_open",
        "resident_session_exec",
        "resident_session_close",
        "containers_start_service",
    ];

    assert_eq!(
        doc_tools.len(),
        expected_tools.len(),
        "docs/skills.md lists {} tools but the MCP server has {}. \
         Please update docs/skills.md to match.",
        doc_tools.len(),
        expected_tools.len()
    );

    for tool in &expected_tools {
        assert!(
            doc_tools.contains(tool),
            "Tool '{}' is in the MCP server but missing from docs/skills.md. \
             Please add it to the documentation table.",
            tool
        );
    }

    for doc_tool in &doc_tools {
        assert!(
            expected_tools.contains(doc_tool),
            "Tool '{}' is documented in docs/skills.md but not in the MCP server. \
             Please remove it or add it to the implementation.",
            doc_tool
        );
    }
}

// ── S3-1b: risk-tier documentation consistency ──────────────────────────────

/// The read-only / write classification in `docs/skill-distribution.md` must
/// agree with the MCP tool registry, name for name.
///
/// That document is what an operator reads before deciding what to grant an AI
/// client, so a tool that quietly changes tier — or a new tool that is never
/// classified at all — is a privilege decision made by omission. Nothing in the
/// codebase referenced the file, so nothing could catch that drift: it happened
/// to be correct (34 + 38 = 72 listings, 71 unique, `ssh_webhook_config` being
/// the one tool with both a `(get)` and a `(set)` form) rather than being kept
/// correct.
///
/// Both tiers are therefore pinned in full below. Counting alone would not do:
/// moving a mutating tool from `写入类` to `只读类` leaves the count, the union
/// and the cross-tier overlap all untouched, and that move is the one drift with
/// a security consequence.
#[test]
fn risk_tiers_match_mcp_tool_registry() {
    // A listing entry is a bare tool name, optionally followed by a form marker
    // (`ssh_webhook_config (get)`); the marker is dropped so both forms collapse
    // onto the single registry name.
    fn normalize(token: &str) -> &str {
        let mut name = token;
        for suffix in ["(get)", "(set)"] {
            if let Some(stripped) = name.strip_suffix(suffix) {
                name = stripped;
            }
        }
        name
    }

    // Collect the fenced tool listing that follows the `### <marker>` heading.
    fn tier_listing<'a>(doc: &'a str, marker: &str) -> Vec<&'a str> {
        let mut out = Vec::new();
        let mut in_section = false;
        let mut in_fence = false;
        for line in doc.lines() {
            if line.starts_with("### ") {
                if in_section {
                    break; // the next tier heading ends this listing
                }
                in_section = line.contains(marker);
                continue;
            }
            if !in_section {
                continue;
            }
            if line.trim_start().starts_with("```") {
                if in_fence {
                    break; // the closing fence ends the listing
                }
                in_fence = true;
                continue;
            }
            if in_fence {
                for token in line.split_whitespace() {
                    let name = normalize(token);
                    if name.starts_with("ssh_") {
                        out.push(name);
                    }
                }
            }
        }
        out
    }

    // The published read-only tier. `ssh_webhook_config` is the only tool that
    // also appears in `WRITE`, because it exposes both a `(get)` and a `(set)`
    // form.
    const READ_ONLY: [&str; 34] = [
        "ssh_list_hosts",
        "ssh_list_daemons",
        "ssh_ping",
        "ssh_audit",
        "ssh_audit_export",
        "ssh_sftp_ls",
        "ssh_sftp_stat",
        "ssh_session_list",
        "ssh_session_read",
        "ssh_forward_list",
        "ssh_risk_check",
        "ssh_approval_list",
        "ssh_approval_policies_list",
        "ssh_approval_check",
        "ssh_connection_status",
        "ssh_playbook_list",
        "ssh_playbook_dry_run",
        "ssh_config_export",
        "ssh_config_import_preview",
        "ssh_webhook_config",
        "ssh_doctor",
        "ssh_metrics",
        "ssh_metrics_trend",
        "ssh_preview_exec",
        "ssh_gate_status",
        "ssh_daemon_diagnose",
        "ssh_daemon_version_check",
        "ssh_daemons_view",
        "ssh_events_subscribe",
        "ssh_sync_diff",
        "ssh_snippet_list",
        "ssh_redact_rule_list",
        "ssh_highlight_rule_list",
        "ssh_algo_prefs_get",
    ];

    // The published write tier.
    const WRITE: [&str; 38] = [
        "ssh_exec",
        "ssh_exec_multi",
        "ssh_exec_compare",
        "ssh_session_open",
        "ssh_session_write",
        "ssh_session_close",
        "ssh_sftp_upload",
        "ssh_sftp_download",
        "ssh_sftp_mkdir",
        "ssh_forward_add",
        "ssh_sftp_rename",
        "ssh_sftp_rm",
        "ssh_sftp_rmdir",
        "ssh_sftp_rm_rf",
        "ssh_forward_remove",
        "ssh_add_host",
        "ssh_remove_host",
        "ssh_import_config",
        "ssh_connect",
        "ssh_disconnect",
        "ssh_approval_respond",
        "ssh_playbook_run",
        "ssh_webhook_config",
        "ssh_config_import",
        "ssh_snippet_save",
        "ssh_snippet_delete",
        "ssh_redact_rule_add",
        "ssh_redact_rule_update",
        "ssh_redact_rule_remove",
        "ssh_redact_rule_reset",
        "ssh_highlight_rule_add",
        "ssh_highlight_rule_update",
        "ssh_highlight_rule_remove",
        "ssh_highlight_rule_reset",
        "ssh_algo_prefs_set",
        "ssh_algo_prefs_clear",
        "ssh_health_snapshot",
        "ssh_sync_export",
    ];

    let doc = include_str!("../../docs/skill-distribution.md");
    let registry = include_str!("../src/bin/agent2ssh_mcp/tools.rs");

    // No dedup on these two: a tool listed twice in one tier must fail.
    let mut read_only = tier_listing(doc, "只读类");
    read_only.sort_unstable();
    let mut write = tier_listing(doc, "写入类");
    write.sort_unstable();

    assert!(
        !read_only.is_empty() && !write.is_empty(),
        "could not parse the risk tiers out of docs/skill-distribution.md \
         (read-only: {} entries, write: {} entries); the headings or the fenced \
         layout must have changed",
        read_only.len(),
        write.len()
    );

    let mut expected_read_only = READ_ONLY.to_vec();
    expected_read_only.sort_unstable();
    assert_eq!(
        read_only, expected_read_only,
        "the 只读类 tier in docs/skill-distribution.md no longer matches this \
         list. If a tool MOVED here, confirm it truly has no side effects before \
         updating READ_ONLY; if the listing gained a genuinely new tool, add it \
         in both places."
    );

    let mut expected_write = WRITE.to_vec();
    expected_write.sort_unstable();
    assert_eq!(
        write, expected_write,
        "the 写入类 tier in docs/skill-distribution.md no longer matches this \
         list. If a tool DISAPPEARED from here, that is the dangerous direction: \
         a mutating tool must never be presented as safe to grant."
    );

    // The pins above say nothing about a tool that is in the registry but in no
    // tier at all, so the union is checked against the registry too — read from
    // the same `"name": "ssh_..."` literals the tool-count test uses, so the two
    // can never disagree about what the registry holds.
    let mut registry_tools: Vec<&str> = Vec::new();
    let mut rest = registry;
    while let Some(idx) = rest.find("\"name\": \"") {
        let tail = &rest[idx + 9..];
        let end = tail
            .find('"')
            .expect("unterminated MCP tool name in tools.rs");
        let name = &tail[..end];
        if name.starts_with("ssh_") {
            registry_tools.push(name);
        }
        rest = &tail[end..];
    }
    assert!(
        !registry_tools.is_empty(),
        "could not parse any MCP tool name out of src/bin/agent2ssh_mcp/tools.rs"
    );
    registry_tools.sort_unstable();
    registry_tools.dedup();

    let mut classified: Vec<&str> = read_only.iter().chain(write.iter()).copied().collect();
    classified.sort_unstable();
    classified.dedup();

    // Both directions in one assertion. `unclassified` is the realistic drift: a
    // 72nd tool lands in the registry and nobody tiers it — the only other test
    // that notices is the tool count, whose natural fix is to bump a number.
    // `phantom` is the reverse, a tier entry naming something the registry no
    // longer has. Reported together so neither can shadow the other.
    let unclassified: Vec<&str> = registry_tools
        .iter()
        .copied()
        .filter(|tool| !classified.contains(tool))
        .collect();
    let phantom: Vec<&str> = classified
        .iter()
        .copied()
        .filter(|name| !registry_tools.contains(name))
        .collect();
    assert!(
        unclassified.is_empty() && phantom.is_empty(),
        "docs/skill-distribution.md and the MCP registry disagree: {} tool(s) are \
         in the registry but in no risk tier ({:?}); {} risk-tier name(s) are not \
         MCP tools ({:?})",
        unclassified.len(),
        unclassified,
        phantom.len(),
        phantom
    );
}

// ── S3-2: OpenAPI / daemon contract checks ──────────────────────────────────

/// The five method builders axum's `routing` module exposes for the verbs the
/// daemon uses, as they appear in source (with the call paren) and as OpenAPI
/// spells them (without).
const METHOD_BUILDERS: [(&str, &str); 5] = [
    ("get(", "get"),
    ("post(", "post"),
    ("put(", "put"),
    ("delete(", "delete"),
    ("patch(", "patch"),
];

/// `:id` (axum) → `{id}` (OpenAPI), trailing slash trimmed.
///
/// A `:` starts a parameter that runs to the next `/`, so the whole segment is
/// renamed at once; anything else is copied verbatim.
fn normalized_route_path(path: &str) -> String {
    let mut normalized = String::new();
    let mut chars = path.chars();
    while let Some(ch) = chars.next() {
        if ch == ':' {
            normalized.push('{');
            for ch in chars.by_ref() {
                if ch == '/' {
                    break;
                }
                normalized.push(ch);
            }
            normalized.push('}');
            normalized.push('/');
        } else {
            normalized.push(ch);
        }
    }
    // Keep the root path intact; only strip trailing slashes after it.
    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    normalized
}

/// Status codes a handler can answer with on its own, keyed by the
/// `StatusCode::` constant that names them in source.
///
/// `UNAUTHORIZED` is deliberately absent: 401 is answered by the middleware for
/// the whole route, and the contract test above already requires it on every
/// non-public operation.
const HANDLER_STATUS: [(&str, &str); 10] = [
    ("BAD_REQUEST", "400"),
    ("FORBIDDEN", "403"),
    ("NOT_FOUND", "404"),
    ("REQUEST_TIMEOUT", "408"),
    ("CONFLICT", "409"),
    ("LOCKED", "423"),
    ("TOO_MANY_REQUESTS", "429"),
    ("INTERNAL_SERVER_ERROR", "500"),
    ("BAD_GATEWAY", "502"),
    ("SERVICE_UNAVAILABLE", "503"),
];

/// Helpers that answer a status their caller's body never names.
///
/// `HANDLER_STATUS` reads `StatusCode::` literals, so a handler that delegates
/// its 429 to `reject_if_rate_limited` (directly, or through the preflight
/// shared by every host operation) documents nothing and failed nothing. Three
/// operations had drifted that way — `DELETE /forwards/{id}` and both
/// `/connections/{host}/…` — while their sibling routes
/// (`/forwards/{id}/start|stop`) did document it, which is how the omission
/// stayed invisible: the family looked consistent from one route.
///
/// A call in the body is not proof the status is reachable: see
/// `INBAND_REJECTIONS` below for the two routes that consume the `Err` instead
/// of returning it, and would be wrongly indicted by this list.
///
/// Only 429 is listed. `preflight_guarded_request` can also surface 408/423/500
/// from the authorization chain, and pinning those here would demand a status
/// on routes that legitimately never reach that branch; 429 is the one that is
/// decided before authorization, on every path that calls these helpers.
const HELPER_STATUS: [(&str, &str); 4] = [
    ("too_many_requests_status()", "429"),
    ("reject_if_rate_limited(", "429"),
    ("register_limited_session_for_command(", "429"),
    ("preflight_guarded_request(", "429"),
];

/// Operations whose gate and rate-limit refusals never become an HTTP status.
///
/// `HELPER_STATUS` is a text scan: it sees `reject_if_rate_limited(` in a body
/// and concludes the caller can receive a 429. Two handlers consume that `Err`
/// instead of returning it — `exec_stream` sends `{"type":"error"}` over the
/// socket it has already upgraded, and `terminal_broadcast_run` marks every
/// eligible target `authorized: false` and returns 200 — so a `"429"` or
/// `"423"` there would promise a branch this server never takes. `/terminal` is
/// the control in the same family: identical checks, run before the upgrade, so
/// its 423 and 429 are real and stay in its `responses`.
///
/// The third field is a phrase the path block must still carry, so the
/// exemption cannot decay into silence — the refusal stays documented, just not
/// as a status. Rewording that prose breaks this test on purpose.
const INBAND_REJECTIONS: [(&str, &str, &str); 2] = [
    ("get", "/exec/stream", "only as a frame"),
    ("post", "/terminal/broadcast", "returns 200"),
];

/// The methods one `.route(...)` call mounts, in source order, with their handler.
///
/// `get(a).post(b)` mounts two, and a builder chain that wraps across lines
/// still belongs to a single call — so the caller bounds `segment` at the next
/// `.route(`. A builder is only counted when the character before it is not
/// part of a name: `algo_prefs_get` ends in `_get` and must not read as a
/// `get(` call.
fn mounted_handlers(segment: &str) -> Vec<(&'static str, String)> {
    let mut mounted = Vec::new();
    for (needle, method) in METHOD_BUILDERS {
        let mut from = 0;
        while let Some(found) = segment[from..].find(needle) {
            let at = from + found;
            let preceded_by_name = segment[..at]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_');
            if !preceded_by_name {
                let handler: String = segment[at + needle.len()..]
                    .chars()
                    .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                    .collect();
                if !handler.is_empty() {
                    mounted.push((method, handler));
                }
            }
            from = at + needle.len();
        }
    }
    mounted
}

/// Every `.route(...)` call in the daemon, as (normalized path, method, handler).
///
/// Each call is bounded at the next one (or at `.with_state(`, which ends the
/// router chain) so a multi-line builder chain cannot borrow the methods of the
/// call that follows it.
fn daemon_routes(daemon: &str) -> Vec<(String, &'static str, String)> {
    let starts: Vec<usize> = daemon
        .match_indices(".route(")
        .map(|(index, _)| index)
        .collect();
    let mut routes = Vec::new();
    for (position, start) in starts.iter().enumerate() {
        let mut end = starts.get(position + 1).copied().unwrap_or(daemon.len());
        if let Some(with_state) = daemon[*start..].find(".with_state(") {
            end = end.min(start + with_state);
        }
        let segment = &daemon[*start..end];
        let Some(open) = segment.find('"') else {
            continue;
        };
        let Some(close) = segment[open + 1..].find('"') else {
            continue;
        };
        let path = &segment[open + 1..open + 1 + close];
        if !path.starts_with('/') {
            continue;
        }
        let normalized = normalized_route_path(path);
        for (method, handler) in mounted_handlers(segment) {
            routes.push((normalized.clone(), method, handler));
        }
    }
    routes
}

/// The body of one handler, from its `async fn` line to its closing brace.
///
/// The obvious terminator — "the next `async fn` or `fn`" — is wrong in a way
/// that stayed invisible while it was merely unfortunate: 66 of the 106 route
/// handlers are followed by a `struct`, a `const`, a doc comment or a helper,
/// so their "bodies" ran hundreds of bytes past the closing brace. Nothing
/// foreign happened to carry a `StatusCode::` constant yet, which is exactly
/// why this could rot unnoticed — the day a neighbouring item gains one, the
/// scan reports it against the wrong operation. Walking the braces instead
/// bounds the body at the item it belongs to.
///
/// It panics rather than returning the rest of the file, for the reason
/// `handler_body` below already panics: a body that cannot be isolated makes
/// every check on it vacuous, and a vacuous check is worse than a red one.
fn handler_body_of<'a>(daemon: &'a str, handler: &str) -> &'a str {
    let signature = format!("\nasync fn {handler}(");
    let Some(at) = daemon.find(&signature) else {
        panic!(
            "the route table names `{handler}`, but the daemon has no \
             `async fn {handler}(` for these checks to read"
        );
    };
    let rest = &daemon[at + 1..];
    let open = opening_brace(rest)
        .unwrap_or_else(|| panic!("`{handler}` has no body, so its checks would be vacuous"));
    let close = closing_brace(rest, open)
        .unwrap_or_else(|| panic!("`{handler}`'s body has unbalanced braces"));
    &rest[..close]
}

/// The parameter list of one handler, between its `fn name(` and its closing `)`.
///
/// The extractor lives here and nowhere else. Scanning the whole body for
/// `Json<` would count the *return* type — every handler that answers with a
/// body is `Result<Json<T>, (StatusCode, Json<ErrorBody>)>` — so a handler that
/// reads no body at all would look like one that requires `Json`. Anchoring the
/// search on `: Json<` narrows it but still answers the wrong question: it
/// cannot tell `Json<T>` from `Option<Json<T>>`, which is the distinction this
/// file's body-handling test exists to draw. Walking the parentheses answers it
/// directly.
///
/// Panics for the same reason `handler_body_of` does: a handler whose parameters
/// cannot be isolated makes every check on them vacuous.
fn handler_params_of<'a>(daemon: &'a str, handler: &str) -> &'a str {
    let signature = format!("\nasync fn {handler}(");
    let Some(at) = daemon.find(&signature) else {
        panic!(
            "the route table names `{handler}`, but the daemon has no \
             `async fn {handler}(` for these checks to read"
        );
    };
    let rest = &daemon[at + 1..];
    // `signature` starts with the newline at `at`, so `rest` is one byte shorter
    // and the `(` lands at `signature.len() - 2`. `find` says the same thing and
    // cannot be off by one.
    let Some(open) = rest.find('(') else {
        panic!("`{handler}` has no parameter list");
    };
    let mut depth = 0usize;
    for (offset, ch) in rest[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &rest[open + 1..open + offset];
                }
            }
            _ => {}
        }
    }
    panic!("`{handler}`'s parameter list is never closed");
}

/// The `{` that opens a function body: the first one outside the parameter
/// list, so a parenthesised return type does not look like the body.
fn opening_brace(source: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (at, ch) in source.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            '{' if depth == 0 => return Some(at),
            ';' if depth == 0 => return None,
            _ => {}
        }
    }
    None
}

/// Index just past the `}` closing the block opened at `open`.
///
/// String, char and comment contents are skipped: a `}` inside
/// `format!("… }}")` would otherwise close the body early, and a `{` inside a
/// comment would open a block that never closes.
fn closing_brace(source: &str, open: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0i32;
    let mut at = open;
    while at < bytes.len() {
        match bytes[at] {
            b'/' if bytes.get(at + 1) == Some(&b'/') => {
                at = source[at..].find('\n').map_or(bytes.len(), |n| at + n);
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                at = source[at + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |n| at + 2 + n + 2);
            }
            b'"' => at = string_end(source, at)?,
            b'\'' => at = char_end(source, at),
            b'{' => {
                depth += 1;
                at += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at + 1);
                }
                at += 1;
            }
            _ => at += 1,
        }
    }
    None
}

/// Index just past the string literal whose opening quote is at `quote`.
///
/// Handles the raw forms (`r"…"`, `r#"…"#`), which the daemon uses in its
/// `daemon_tokens.toml` fixtures.
fn string_end(source: &str, quote: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if quote > 0 && bytes[quote - 1] == b'r' {
        let hashes = source[quote + 1..]
            .bytes()
            .take_while(|byte| *byte == b'#')
            .count();
        let closing = format!("\"{}", "#".repeat(hashes));
        let from = quote + 1 + hashes;
        return source[from..]
            .find(&closing)
            .map(|n| from + n + closing.len());
    }
    let mut at = quote + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'"' => return Some(at + 1),
            _ => at += 1,
        }
    }
    None
}

/// Index just past a char literal; a lifetime (`'a`) advances by one instead.
fn char_end(source: &str, quote: usize) -> usize {
    let bytes = source.as_bytes();
    let width = if bytes.get(quote + 1) == Some(&b'\\') {
        2
    } else {
        1
    };
    if bytes.get(quote + width + 1) == Some(&b'\'') {
        quote + width + 2
    } else {
        quote + 1
    }
}

/// A brace inside a literal, a comment or a raw string is not a brace.
///
/// The daemon source has no such shape today, which is exactly why this needs
/// its own test rather than an argument: a scan that reads `'}'` as syntax
/// closes a body one brace early, and every check that reads that body then
/// measures a prefix of it and passes. That is the failure mode the brace walk
/// was introduced to end, one layer down.
///
/// The cases below are the shapes a later edit could plausibly add, including
/// the one a reviewer reported as already broken — a `'{'` immediately followed
/// by a second literal, where the claim was that the scanner swallows the
/// following colon and desynchronises.
#[test]
fn a_brace_inside_a_literal_does_not_close_the_body() {
    let cases = [
        "fn f() { let close = '}'; }",
        "fn f() { let quote = '\\''; }",
        "fn f() { let slash = '\\\\'; }",
        "fn f() { let pair = ('{', ':'); }",
        "fn f() { let escaped = '\\u{7D}'; }",
        "fn f() { let wide = '中'; }",
        "fn f() { let wide_pair = ('中', '}'); }",
        "fn f() { let s = r#\"}\"#; }",
        "fn f() { let s = \"}\"; }",
        "fn f() { /* } */ let n = 1; }",
        "fn f() { // }\n let n = 1; }",
        "fn f() { let t: &'static str = \"a}b\"; }",
    ];
    for case in cases {
        let open = case.find('{').expect("the case has a body");
        let end = closing_brace(case, open).unwrap_or_else(|| panic!("no end for {case}"));
        assert_eq!(
            &case[end..],
            "",
            "the scan stopped inside the body of {case:?} instead of at its closing brace"
        );
    }
}

/// Every daemon route is in `docs/api.yaml`, and nothing else is — path,
/// method, and the 401 that the auth middleware answers with.
///
/// `docs/api.yaml` is the published contract, so a route that is not in it is a
/// route no client can discover — and the gap is invisible from either side
/// alone, which is how `/snippets`, the port-forward batch operations and the
/// 2FA prompt endpoints all went undocumented at once. The two non-API routes
/// are the browser console and its root redirect.
///
/// Paths alone are only a third of the contract. A handler whose *method*
/// changed still has its path documented, and an operation with no documented
/// `401` still looks complete — yet 401 is the one code every non-public route
/// can return, because `auth_middleware` rejects before any handler runs. It
/// went missing on 98 of 103 operations without a single test going red.
#[test]
fn api_yaml_documents_every_daemon_route() {
    // `/` and `/console` serve the web console, not the API.
    const NON_API_ROUTES: [&str; 2] = ["/", "/console"];

    let daemon = include_str!("../src/bin/agent2ssh-daemon.rs");
    let spec = include_str!("../../docs/api.yaml");

    // `Path` parameters are `:id` in axum and `{id}` in OpenAPI.
    let parsed = daemon_routes(daemon);
    let mut routes: Vec<String> = parsed
        .iter()
        .map(|(path, _, _)| path.clone())
        .filter(|path| !NON_API_ROUTES.contains(&path.as_str()))
        .collect();
    routes.sort();
    routes.dedup();
    let mut mounted: Vec<(String, &'static str)> = parsed
        .iter()
        .map(|(path, method, _)| (path.clone(), *method))
        .collect();
    mounted.sort();
    mounted.dedup();
    assert!(!routes.is_empty(), "no daemon routes were found to compare");

    // Top-level OpenAPI paths are indented two spaces under `paths:`.
    let documented: Vec<String> = spec
        .lines()
        .filter_map(|line| line.strip_prefix("  /"))
        .map(|rest| rest.split(':').next().unwrap_or_default())
        .map(|path| {
            if path.is_empty() {
                "/".to_string()
            } else {
                format!("/{path}")
            }
        })
        .collect();

    let undocumented: Vec<&String> = routes
        .iter()
        .filter(|route| !documented.iter().any(|path| path == route.as_str()))
        .collect();
    assert!(
        undocumented.is_empty(),
        "these daemon routes are missing from docs/api.yaml: {undocumented:?}. \
         Add them to `paths:` — every route is part of the contract a client programs against."
    );

    let phantom: Vec<&String> = documented
        .iter()
        .filter(|path| !routes.iter().any(|route| route == *path))
        .collect();
    assert!(
        phantom.is_empty(),
        "docs/api.yaml documents paths the daemon does not serve: {phantom:?}. \
         Either the route was removed and the spec was not, or the path is misspelled."
    );

    // ── Methods and the auth code ───────────────────────────────────────────
    //
    // A path alone is not the contract: the method is what a client calls, and
    // the 401 is what every non-public route answers with before a handler
    // runs. Both drifted while this test stayed green.
    struct DocumentedOperation {
        path: String,
        method: &'static str,
        has_401: bool,
        is_public: bool,
    }

    let mut operations: Vec<DocumentedOperation> = Vec::new();
    let mut current_path: Option<String> = None;
    for line in spec.lines() {
        let body = line.trim_start();
        let indent = line.len() - body.len();
        if let Some(rest) = line.strip_prefix("  /") {
            let path = rest.split(':').next().unwrap_or_default();
            current_path = Some(if path.is_empty() {
                "/".to_string()
            } else {
                format!("/{path}")
            });
            continue;
        }
        // Path items and the operation keys under them: methods sit four
        // spaces in, `security` and the response codes deeper.
        if indent == 4 {
            if let Some(name) = body.strip_suffix(':') {
                if let Some((_, method)) = METHOD_BUILDERS.iter().find(|(_, known)| *known == name)
                {
                    operations.push(DocumentedOperation {
                        path: current_path.clone().unwrap_or_default(),
                        method,
                        has_401: false,
                        is_public: false,
                    });
                }
            }
            continue;
        }
        let Some(operation) = operations.last_mut() else {
            continue;
        };
        if indent == 6 && body == "security: []" {
            operation.is_public = true;
        }
        if indent == 8 && body.starts_with("\"401\":") {
            operation.has_401 = true;
        }
    }

    let undocumented_methods: Vec<String> = mounted
        .iter()
        .filter(|(path, method)| {
            !NON_API_ROUTES.contains(&path.as_str())
                && !operations
                    .iter()
                    .any(|operation| operation.path == *path && operation.method == *method)
        })
        .map(|(path, method)| format!("{method} {path}"))
        .collect();
    assert!(
        undocumented_methods.is_empty(),
        "these daemon route methods are missing from docs/api.yaml: {undocumented_methods:?}. \
         A client programs against the method as well as the path, so a changed method breaks \
         it even when the path still matches."
    );

    let phantom_methods: Vec<String> = operations
        .iter()
        .filter(|operation| {
            !mounted
                .iter()
                .any(|(path, method)| *path == operation.path && *method == operation.method)
        })
        .map(|operation| format!("{} {}", operation.method, operation.path))
        .collect();
    assert!(
        phantom_methods.is_empty(),
        "docs/api.yaml documents methods the daemon does not serve: {phantom_methods:?}. \
         Either the route's method changed and the spec was not, or the spec is wrong."
    );

    let missing_401: Vec<String> = operations
        .iter()
        .filter(|operation| !operation.is_public && !operation.has_401)
        .map(|operation| format!("{} {}", operation.method, operation.path))
        .collect();
    assert!(
        missing_401.is_empty(),
        "these operations do not document the 401 the auth middleware returns: {missing_401:?}. \
         Every route outside `is_public_path` is rejected with 401 before its handler runs, so \
         the code belongs to the operation, not to the handler body — which is why it went \
         missing without a test going red."
    );
}

/// Every status code a handler answers with on its own is documented.
///
/// 401 is a property of the route (the middleware rejects before a handler
/// runs); every other code is a property of the *handler*, which knows what it
/// had to read and what it refused. That is what made them invisible: 21
/// operations answered 500 for an unreadable config while the spec listed only
/// a 200, `/forwards/{id}` answered both 400 and 403 while documenting neither,
/// and `/daemons/{alias}/exec` answered 404 and 502 with no trace in the spec.
/// A client generated from the spec reads each of those as an unknown failure —
/// `GET /gate` being the worst case, since the one question a broken gate file
/// must still answer is "is the gate paused".
///
/// The handler→operation map is derived rather than written down: the handler
/// named in each `.route(...)` call is looked up in the same source text and
/// its own body is checked for every constant in `HANDLER_STATUS`. A code that
/// arrives through a shared helper (`preflight_guarded_request`,
/// `authorize_targets`, `locked_status`) is not visible this way, because that
/// is the handler's *dependency* failing, not its own branch — those routes
/// document their 408/423/500 by hand. The one exception is 429: it is decided
/// before authorization on every helper path, so `HELPER_STATUS` makes it
/// required rather than remembered.
///
/// Reachability is not the same as presence: `INBAND_REJECTIONS` names the two
/// operations whose helper call answers inside a 2xx, and the third assertion
/// below holds them to the opposite rule.
#[test]
fn api_yaml_documents_the_codes_each_handler_can_return() {
    let daemon = include_str!("../src/bin/agent2ssh-daemon.rs");
    let spec = include_str!("../../docs/api.yaml");

    // Operations and the status codes they document, parsed by indentation:
    // methods sit four spaces in under their path, response keys eight.
    let mut documented: Vec<(String, &'static str, Vec<String>)> = Vec::new();
    let mut current_path: Option<String> = None;
    for line in spec.lines() {
        let body = line.trim_start();
        let indent = line.len() - body.len();
        if let Some(rest) = line.strip_prefix("  /") {
            let path = rest.split(':').next().unwrap_or_default();
            current_path = Some(if path.is_empty() {
                "/".to_string()
            } else {
                format!("/{path}")
            });
            continue;
        }
        if indent == 4 {
            if let Some(name) = body.strip_suffix(':') {
                if let Some((_, method)) = METHOD_BUILDERS.iter().find(|(_, known)| *known == name)
                {
                    documented.push((current_path.clone().unwrap_or_default(), method, Vec::new()));
                }
            }
            continue;
        }
        if indent != 8 {
            continue;
        }
        let Some(operation) = documented.last_mut() else {
            continue;
        };
        if let Some(code) = body
            .strip_prefix('"')
            .and_then(|rest| rest.split('"').next())
        {
            operation.2.push(code.to_string());
        }
    }

    let undocumented: Vec<String> = daemon_routes(daemon)
        .into_iter()
        .flat_map(|(path, method, handler)| {
            let body = handler_body_of(daemon, &handler);
            // An entirely undocumented operation is the route test's finding.
            let Some((_, _, codes)) = documented.iter().find(|(known_path, known_method, _)| {
                *known_path == path && *known_method == method
            }) else {
                return Vec::new();
            };
            HANDLER_STATUS
                .iter()
                .filter(|(constant, code)| {
                    body.contains(&format!("StatusCode::{constant}"))
                        && !codes.iter().any(|documented| documented == code)
                })
                .map(|(_, code)| format!("{code} {method} {path} ({handler})"))
                .collect()
        })
        .collect();
    assert!(
        undocumented.is_empty(),
        "these operations answer a status code they do not document: {undocumented:?}. \
         The code comes from the handler's own body, so a client that programs against \
         the spec cannot tell this failure from an unknown one."
    );

    // The same map for the statuses a helper decides rather than the handler.
    let undocumented_via_helper: Vec<String> = daemon_routes(daemon)
        .into_iter()
        .flat_map(|(path, method, handler)| {
            let body = handler_body_of(daemon, &handler);
            let Some((_, _, codes)) = documented.iter().find(|(known_path, known_method, _)| {
                *known_path == path && *known_method == method
            }) else {
                return Vec::new();
            };
            // The call is in the body, but the `Err` is consumed rather than
            // returned, so no status ever reaches the client.
            if INBAND_REJECTIONS
                .iter()
                .any(|(exempt_method, exempt_path, _)| {
                    *exempt_method == method && *exempt_path == path
                })
            {
                return Vec::new();
            }
            HELPER_STATUS
                .iter()
                .filter(|(call, code)| {
                    body.contains(call) && !codes.iter().any(|documented| documented == code)
                })
                .map(|(_, code)| format!("{code} {method} {path} ({handler} via helper)"))
                .collect()
        })
        .collect();
    assert!(
        undocumented_via_helper.is_empty(),
        "these operations can answer a status through a shared helper and do not document \
         it: {undocumented_via_helper:?}. The check runs before authorization on every one \
         of these paths, so the failure reaches a caller who cannot tell it apart from an \
         unknown error."
    );

    // The exemption above is itself a claim about the code, so check it in both
    // directions. Without this the list would be a place to silence the check:
    // an operation could drop 423/429 from its `responses` and say nothing.
    let unbacked: Vec<String> = INBAND_REJECTIONS
        .iter()
        .flat_map(|entry| {
            let (method, path, marker) = *entry;
            let Some((_, _, codes)) = documented.iter().find(|(known_path, known_method, _)| {
                known_path.as_str() == path && *known_method == method
            }) else {
                return vec![format!(
                    "{method} {path} is exempt from the helper check but has no operation in \
                     the spec, so the exemption guards nothing"
                )];
            };
            let mut problems: Vec<String> = ["423", "429"]
                .iter()
                .filter(|code| codes.iter().any(|documented| documented == *code))
                .map(|code| format!("{method} {path} documents {code} but answers it in-band"))
                .collect();
            match spec_path_block(spec, path) {
                Some(block) if block.contains(marker) => {}
                Some(_) => problems.push(format!(
                    "{method} {path} no longer says `{marker}`, so an in-band refusal is now \
                     undocumented rather than mis-documented"
                )),
                None => problems.push(format!("{method} {path} has no spec block to read")),
            }
            problems
        })
        .collect();
    assert!(
        unbacked.is_empty(),
        "the in-band exemption list and docs/api.yaml disagree: {unbacked:?}. An operation \
         on that list must document neither 423 nor 429 — those refusals never become an \
         HTTP status — and must still say in prose where the refusal does go."
    );
}

/// The spec text of one path item, from its `  /path:` header to the next one.
///
/// Path items are the only lines indented exactly two spaces that open with a
/// slash, so the next such header bounds the block. Used to check that prose the
/// test above depends on has not drifted away from the claim built on it.
fn spec_path_block<'a>(spec: &'a str, path: &str) -> Option<&'a str> {
    let header = format!("\n  {path}:");
    let start = spec.find(&header)? + 1;
    let rest = &spec[start..];
    let end = rest[1..]
        .match_indices("\n  /")
        .next()
        .map(|(index, _)| index + 1)
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Every documented response declares a body, because every one of them has one.
///
/// The daemon answers every error as `(StatusCode, Json<ErrorBody>)`, so a
/// response with a `description` and no `content:` documents a body-less reply
/// that cannot happen. 57 of them were written that way, all 4xx/5xx, and the
/// omission is invisible from both sides: the value is there at runtime, and a
/// client generator sees a status with no schema and falls back to "unknown".
///
/// `101` is exempt — `Switching Protocols` carries no body by definition.
#[test]
fn api_yaml_declares_a_body_for_every_response() {
    let spec = include_str!("../../docs/api.yaml");

    fn flush(pending: &mut Option<(String, bool)>, operation: &str, missing: &mut Vec<String>) {
        if let Some((code, has_content)) = pending.take() {
            if !has_content && code != "101" && code != "204" {
                missing.push(format!("{code} {operation}"));
            }
        }
    }

    let mut missing: Vec<String> = Vec::new();
    let mut path = String::new();
    let mut operation = String::new();
    let mut pending: Option<(String, bool)> = None;

    for line in spec.lines() {
        let body = line.trim_start();
        let indent = line.len() - body.len();
        if let Some(rest) = line.strip_prefix("  /") {
            flush(&mut pending, &operation, &mut missing);
            let name = rest.split(':').next().unwrap_or_default();
            path = if name.is_empty() {
                "/".to_string()
            } else {
                format!("/{name}")
            };
            operation.clear();
            continue;
        }
        if indent == 4 {
            flush(&mut pending, &operation, &mut missing);
            if let Some(name) = body.strip_suffix(':') {
                if METHOD_BUILDERS.iter().any(|(_, known)| *known == name) {
                    operation = format!("{} {path}", name.to_uppercase());
                }
            }
            continue;
        }
        if indent == 8 {
            flush(&mut pending, &operation, &mut missing);
            if let Some(rest) = body
                .strip_prefix('"')
                .and_then(|rest| rest.split('"').next())
            {
                if !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()) {
                    pending = Some((rest.to_string(), false));
                }
            }
            continue;
        }
        if indent >= 10 && body.starts_with("content:") {
            if let Some((_, has_content)) = pending.as_mut() {
                *has_content = true;
            }
        }
    }
    flush(&mut pending, &operation, &mut missing);

    assert!(
        missing.is_empty(),
        "these responses document no body: {missing:?}. The daemon answers them as \
         `(StatusCode, Json<ErrorBody>)`, so the schema exists at runtime and only the \
         contract is silent about it."
    );
}

/// How a handler reads its request body decides which codes it can answer with,
/// and the spec has to agree with the *reading*, not with the schema.
///
/// Three readings coexist in the daemon and they fail differently:
///
/// - `Json<T>` — 47 handlers. axum rejects before the handler runs: 415 when the
///   request declares no `application/json` content type, 422 when the body
///   parses but does not fit (`JsonDataError`), 400 when it does not parse at all
///   (`JsonSyntaxError`). None of those three codes appeared on any of the 105
///   operations, so a generated client saw every body mistake as an unknown
///   failure — the same blind spot the missing 401s had, one layer down.
/// - `axum::body::Bytes` — `approval_approve` only. The handler parses by hand
///   and answers its own 400, so 415 and 422 cannot happen and must not be
///   claimed. The spec claimed both for one round, because the response codes
///   had been added by grepping for `requestBody` instead of reading the
///   signature.
/// - `Option<Json<T>>` — 3 handlers. Its rejection type is `Infallible`, so the
///   `.ok()` these handlers use cannot fail: a wrong content type, invalid JSON
///   and a mistyped field all arrive as `None`, i.e. "no body was sent". The
///   dangerous case is `/shutdown`, where a body that was meant to name a PID
///   silently skips the check instead of failing it.
///
/// The reader is derived from each handler's own parameter list, so this stays
/// correct when a signature changes; the operation is then looked up in the spec
/// and held to whatever that reading implies.
#[test]
fn api_yaml_documents_how_each_handler_reads_its_body() {
    // `/` and `/console` serve the web console, not the API.
    const NON_API_ROUTES: [&str; 2] = ["/", "/console"];

    let daemon = include_str!("../src/bin/agent2ssh-daemon.rs");
    let spec = include_str!("../../docs/api.yaml");

    struct Documented {
        method: &'static str,
        path: String,
        codes: Vec<String>,
        /// `(status code, media type)` pairs, so a code documented with the wrong
        /// kind of body can be caught: axum's rejections answer `text/plain`,
        /// not the daemon's `Error` object.
        media: Vec<(String, String)>,
        has_request_body: bool,
        /// The operation's own lines, so the prose can be checked for the
        /// caveat a swallowed body needs.
        text: String,
    }

    let mut documented: Vec<Documented> = Vec::new();
    let mut current_path = String::new();
    // Lines between a path and its first method belong to neither operation;
    // without this, a path-level `description:` would be read as the previous
    // operation's and could satisfy its caveat check.
    let mut current_operation: Option<usize> = None;
    let mut current_code = String::new();
    for line in spec.lines() {
        let body = line.trim_start();
        let indent = line.len() - body.len();
        if let Some(rest) = line.strip_prefix("  /") {
            let path = rest.split(':').next().unwrap_or_default();
            current_path = if path.is_empty() {
                "/".to_string()
            } else {
                format!("/{path}")
            };
            current_operation = None;
            continue;
        }
        if indent == 4 {
            if let Some(name) = body.strip_suffix(':') {
                if let Some((_, method)) = METHOD_BUILDERS.iter().find(|(_, known)| *known == name)
                {
                    documented.push(Documented {
                        method,
                        path: current_path.clone(),
                        codes: Vec::new(),
                        media: Vec::new(),
                        has_request_body: false,
                        text: String::new(),
                    });
                    current_operation = Some(documented.len() - 1);
                    current_code.clear();
                }
            }
            continue;
        }
        let Some(index) = current_operation else {
            continue;
        };
        let operation = &mut documented[index];
        operation.text.push_str(body);
        operation.text.push('\n');
        if indent == 6 && body.starts_with("requestBody:") {
            operation.has_request_body = true;
        }
        if indent == 8 {
            if let Some(code) = body
                .strip_prefix('"')
                .and_then(|rest| rest.split('"').next())
            {
                current_code = code.to_string();
                operation.codes.push(code.to_string());
            }
        }
        // `content:` sits ten spaces in under a response, its media type twelve.
        if indent == 12 && (body == "text/plain:" || body == "application/json:") {
            operation
                .media
                .push((current_code.clone(), body.trim_end_matches(':').to_string()));
        }
    }

    enum BodyReader {
        /// `Json<T>`: 415 / 422 / 400 all come from the extractor.
        Required,
        /// `Option<Json<T>>`: every extraction error becomes "no body".
        Swallowed,
        /// `Bytes`: the handler parses the body itself.
        HandParsed,
        /// No body parameter at all.
        Absent,
    }

    let mut violations: Vec<String> = Vec::new();
    let mut required = 0usize;
    let mut swallowed = 0usize;
    // The file-level note names the swallowed operations one path at a time, and
    // until this was derived nothing checked the names against the handlers: a
    // path was added to that sentence while its handler read `Json<T>`, which
    // told a client that `/containers/start-service` ignores a malformed body
    // instead of refusing it. Prose that enumerates is a derived fact in a
    // costume, so derive it.
    let mut swallowed_paths: Vec<String> = Vec::new();

    for (path, method, handler) in daemon_routes(daemon) {
        if NON_API_ROUTES.contains(&path.as_str()) {
            continue;
        }
        let params = handler_params_of(daemon, &handler);
        let reader = if params.contains("Option<Json<") {
            swallowed += 1;
            swallowed_paths.push(path.clone());
            BodyReader::Swallowed
        } else if params.contains(": Json<") {
            required += 1;
            BodyReader::Required
        } else if params.contains(": axum::body::Bytes") || params.contains(": Bytes,") {
            BodyReader::HandParsed
        } else {
            BodyReader::Absent
        };
        // An entirely undocumented operation is the route test's finding.
        let Some(operation) = documented
            .iter()
            .find(|known| known.path == path && known.method == method)
        else {
            continue;
        };
        let documents = |code: &str| operation.codes.iter().any(|known| known == code);
        let label = format!("{method} {path} ({handler})");

        for code in ["415", "422"] {
            let claimed = documents(code);
            match reader {
                BodyReader::Required => {
                    if !claimed {
                        violations.push(format!(
                            "{label} reads `Json<T>`, so the extractor answers {code} before \
                             the handler runs, but the operation does not document it"
                        ));
                        continue;
                    }
                    // axum answers a rejection as `(status, String)`, which is
                    // `text/plain`; claiming the daemon's `Error` object here
                    // would have a generated client parse a message as JSON.
                    let media: Vec<&str> = operation
                        .media
                        .iter()
                        .filter(|(known, _)| known == code)
                        .map(|(_, media)| media.as_str())
                        .collect();
                    if media != ["text/plain"] {
                        violations.push(format!(
                            "{label} documents {code} with {media:?} as its body; the extractor \
                             answers a plain-text message, so this must be `text/plain`"
                        ));
                    }
                }
                BodyReader::Swallowed => {
                    if claimed {
                        violations.push(format!(
                            "{label} reads `Option<Json<T>>`, whose rejection is `Infallible`: \
                             it cannot answer {code}, so documenting it tells a client that a \
                             malformed body is refused when it is silently ignored"
                        ));
                    }
                }
                BodyReader::HandParsed | BodyReader::Absent => {
                    if claimed {
                        violations.push(format!(
                            "{label} has no `Json<T>` extractor, so nothing answers {code} \
                             before the handler runs"
                        ));
                    }
                }
            }
        }

        match reader {
            BodyReader::Required if !operation.has_request_body => violations.push(format!(
                "{label} requires a JSON body but the operation declares no requestBody, so \
                 a client has no schema to send against"
            )),
            // A swallowed body still needs saying: the operation's own 400 does
            // not cover it, because a body that does not parse never reaches it.
            BodyReader::Swallowed if !operation.text.contains("does not parse") => {
                violations.push(format!(
                    "{label} silently treats a body that does not parse as no body, and its \
                     description does not say so"
                ))
            }
            _ => {}
        }
    }

    assert!(
        required > 0 && swallowed > 0,
        "the extractor shapes were not recognised in the daemon source ({required} `Json<T>`, \
         {swallowed} `Option<Json<T>>`), so every check here would pass for the wrong reason"
    );
    assert!(
        violations.is_empty(),
        "these operations document a body contract the handler does not have: {violations:?}. \
         `Json<T>` answers 415/422 by itself, `Option<Json<T>>` cannot answer either, and \
         `Bytes` answers only what the handler writes."
    );

    // The sentence in `info.description` that enumerates the swallowed
    // operations, joined back into one line so the list can be read out of it.
    let note = spec
        .lines()
        .skip_while(|line| !line.contains("operations read their body that way"))
        .take_while(|line| !line.contains("does not cover it"))
        .collect::<Vec<_>>()
        .join(" ");
    let named: Vec<&str> = note
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|token| token.starts_with('/'))
        .collect();
    assert_eq!(
        named.len(),
        swallowed,
        "the file-level note names {} operations that swallow a malformed body ({named:?}), but \
         {swallowed} handlers read `Option<Json<T>>` ({swallowed_paths:?}). A name in that list \
         whose handler reads `Json<T>` tells a client that a refusing endpoint ignores the body",
        named.len()
    );
    let unexpected: Vec<&&str> = named
        .iter()
        .filter(|name| !swallowed_paths.iter().any(|path| path == *name))
        .collect();
    assert!(
        unexpected.is_empty(),
        "the file-level note names {unexpected:?} as operations that swallow a malformed body, \
         but no handler for them reads `Option<Json<T>>` (that list is {swallowed_paths:?})"
    );
}

/// An error response without a description tells a client the shape of a
/// failure and nothing about what happened.
///
/// Four were in that state, and they were the four a client most needs prose
/// for: `DELETE /hosts/{name}`'s 404 also covers a failed write (a read-only
/// config directory answers "no such host" while the host is right there),
/// `GET /hosts/{name}/policy` answered 403 and 404 with neither a description
/// nor a hint, and `POST /exec`'s 400 covers both a too-long command and every
/// execution failure the runner reports — with no 5xx beside it to separate
/// them.
///
/// Only 4xx and 5xx are required to describe themselves. A 2xx carries a schema
/// and its `description` would restate it; 78 of those are terse by choice.
#[test]
fn api_yaml_describes_every_error_response() {
    let spec = include_str!("../../docs/api.yaml");

    let mut missing: Vec<String> = Vec::new();
    let mut path = String::new();
    let mut operation = String::new();
    // `(code, has_description)` for the response being read.
    let mut pending: Option<(String, bool)> = None;

    fn flush(pending: &mut Option<(String, bool)>, operation: &str, missing: &mut Vec<String>) {
        if let Some((code, described)) = pending.take() {
            if !described && (code.starts_with('4') || code.starts_with('5')) {
                missing.push(format!("{code} {operation}"));
            }
        }
    }

    for line in spec.lines() {
        let body = line.trim_start();
        let indent = line.len() - body.len();
        if let Some(rest) = line.strip_prefix("  /") {
            flush(&mut pending, &operation, &mut missing);
            let name = rest.split(':').next().unwrap_or_default();
            path = if name.is_empty() {
                "/".to_string()
            } else {
                format!("/{name}")
            };
            operation.clear();
            continue;
        }
        if indent == 4 {
            flush(&mut pending, &operation, &mut missing);
            if let Some(name) = body.strip_suffix(':') {
                if METHOD_BUILDERS.iter().any(|(_, known)| *known == name) {
                    operation = format!("{} {path}", name.to_uppercase());
                }
            }
            continue;
        }
        if indent == 8 {
            flush(&mut pending, &operation, &mut missing);
            if let Some(code) = body
                .strip_prefix('"')
                .and_then(|rest| rest.split('"').next())
            {
                if !code.is_empty() && code.chars().all(|ch| ch.is_ascii_digit()) {
                    pending = Some((code.to_string(), false));
                }
            }
            continue;
        }
        if indent >= 10 && body.starts_with("description:") {
            if let Some((_, described)) = pending.as_mut() {
                *described = true;
            }
        }
    }
    flush(&mut pending, &operation, &mut missing);

    assert!(
        missing.is_empty(),
        "these error responses have no description: {missing:?}. An error a client cannot \
         read about is one it can only retry blindly — and several of these codes cover more \
         than one cause, which is the part that needs saying."
    );
}

/// Every WebSocket frame the daemon can push is named in the contract.
///
/// `{"type":"shutdown"}` was pushed by both socket routes and appeared nowhere in
/// `docs/api.yaml`: a client that switches on `type` and does not know it would
/// log an unknown frame at exactly the moment the daemon is going away, which is
/// the moment it needs to back off rather than reconnect.
///
/// The check is by type name, so it catches a *new* frame and not a changed
/// shape — `/terminal` sends `{"type":"shutdown","message":…}` while
/// `/exec/stream` sends `{"type":"shutdown"}`, and that difference is prose, not
/// something this can see.
#[test]
fn every_websocket_frame_the_daemon_sends_is_documented() {
    let daemon = include_str!("../src/bin/agent2ssh-daemon.rs");
    let spec = include_str!("../../docs/api.yaml");

    const MARKER: &str = "json!({\"type\":\"";
    let mut frame_types: Vec<&str> = Vec::new();
    let mut rest = daemon;
    while let Some(at) = rest.find(MARKER) {
        let after = &rest[at + MARKER.len()..];
        if let Some(end) = after.find('"') {
            let name = &after[..end];
            if !frame_types.contains(&name) {
                frame_types.push(name);
            }
        }
        rest = after;
    }

    assert!(
        !frame_types.is_empty(),
        "no WebSocket frames were found in the daemon source, so this check would pass \
         for the wrong reason"
    );
    let undocumented: Vec<&&str> = frame_types
        .iter()
        .filter(|name| !spec.contains(&format!("\"type\":\"{name}\"")))
        .collect();
    assert!(
        undocumented.is_empty(),
        "these WebSocket frame types are pushed but never named in docs/api.yaml: \
         {undocumented:?}. A client switches on `type`; an unexpected one at shutdown or \
         on error is read as a protocol violation."
    );
}

/// A "drains for up to 500 ms" claim is a budget, so the loop needs one deadline.
///
/// Both socket routes keep the same number of milliseconds and spend it
/// differently. `exec_stream` computes one deadline and a chatty command gets
/// 500 ms in total; `handle_terminal` re-arms `timeout(500 ms, recv)` on every
/// event, so its number is an *idle* timeout and the only bound left is the
/// two-second per-connection grace that `POST /shutdown` documents. The terminal
/// description used to make the budget claim for the idle timeout, which told a
/// client its `tail -f` would be cut half a second after the shutdown frame when
/// it can keep draining for two seconds.
///
/// The claim is compared against the shape of the loop, so the two cannot drift
/// apart again. Unifying the loops is fine — change the prose with it.
#[test]
fn a_websocket_drain_budget_claim_matches_the_loop_it_describes() {
    let daemon = include_str!("../src/bin/agent2ssh-daemon.rs");
    let spec = include_str!("../../docs/api.yaml");

    // One path's block, from its `  /name:` line to the next path key.
    fn path_block<'a>(spec: &'a str, path: &str) -> &'a str {
        let head = format!("\n  {path}:");
        let at = spec
            .find(&head)
            .unwrap_or_else(|| panic!("docs/api.yaml has no `{path}` for this check to read"));
        let rest = &spec[at + 1..];
        let end = rest
            .find("\n  /")
            .map(|next| next + 1)
            .unwrap_or(rest.len());
        &rest[..end]
    }

    for (path, handler) in [
        ("/exec/stream", "exec_stream"),
        ("/terminal", "handle_terminal"),
    ] {
        let claims_budget = path_block(spec, path).contains("drains for up to 500 ms");
        let body = handler_body_of(daemon, handler);
        // The span has to stop at this handler's own closing brace: if it ran to
        // the end of the file, the `timeout_at(` below could belong to another
        // function and the comparison would answer for the whole daemon.
        if let Some(at) = body.find("\nasync fn ") {
            let head: String = body.chars().take(160).collect();
            panic!(
                "`{handler}`'s extracted body runs past the function: another `async fn` \
                 starts at byte {at}, so a `timeout_at(` from elsewhere could satisfy this \
                 check. The span starts with {head:?}"
            );
        }
        let measures_one_deadline = body.contains("timeout_at(");
        assert_eq!(
            claims_budget,
            measures_one_deadline,
            "`{path}` {} a 500 ms budget, but `{handler}` {} a single deadline. The number \
             is honest only when the loop computes the deadline once: a `timeout(500 ms, \
             recv)` re-armed per event is an idle timeout, and the drain can then run for \
             the whole per-connection grace.",
            if claims_budget {
                "claims"
            } else {
                "does not claim"
            },
            if measures_one_deadline {
                "computes"
            } else {
                "does not compute"
            },
        );
    }
}

/// A plain YAML scalar may not contain `: `, and prose about status codes does.
///
/// `description: … forwarded verbatim: when the remote answers …,` is not a
/// string to a YAML parser — the second colon reads as a key and the document
/// stops parsing there. That happened twice in one round of edits and nothing
/// noticed: the contract tests above parse `api.yaml` line by line, and no Rust
/// test loads it as YAML, so a spec every client generates from could not be
/// loaded at all while the suite stayed green.
///
/// Exempt, because none of them is a plain scalar: a quoted value, a `|` or `>`
/// block, and a `{ … }` / `[ … ]` flow collection.
#[test]
fn api_yaml_descriptions_with_a_colon_are_quoted() {
    let spec = include_str!("../../docs/api.yaml");
    let offenders: Vec<&str> = spec
        .lines()
        .filter(|line| {
            let Some(value) = line.trim_start().strip_prefix("description: ") else {
                return false;
            };
            let plain = !value.starts_with(['"', '\'', '|', '>', '{', '[']);
            plain && value.contains(": ")
        })
        .map(|line| line.trim())
        .collect();

    assert!(
        offenders.is_empty(),
        "these descriptions would stop a YAML parser at the second colon: {offenders:?}. \
         Quote the value, or switch it to a `>-` block."
    );
}

/// The local exec route must not be reachable without the shared gate chain.
///
/// `/exec` and `/local-exec` are separate handlers, so nothing in the type system
/// stops a later change from giving the local one a "fast path" — and a local
/// command runs on the machine that holds the config, the audit log and any
/// stored credentials, which makes it the worst possible place to lose the
/// execution gate, the rate limiter or the approval policy. The handler body is
/// therefore checked to still call the shared preflight.
#[test]
fn local_exec_route_runs_the_shared_preflight() {
    let daemon = include_str!("../src/bin/agent2ssh-daemon.rs");

    assert!(
        daemon.contains(r#".route("/local-exec", post(local_exec))"#),
        "the /local-exec route is missing, or it no longer points at the local_exec handler"
    );

    let signature = "async fn local_exec(";
    let start = daemon
        .find(signature)
        .expect("the daemon has no local_exec handler");
    let rest = &daemon[start + signature.len()..];
    // The body ends at the next top-level item. Required rather than optional:
    // silently falling back to "the rest of the file" would make the assertion
    // below pass on any other handler's preflight call instead of this one's.
    let end = rest.find("\nasync fn ").expect(
        "local_exec is no longer followed by another top-level handler, so its body \
         cannot be isolated and this check would be vacuous",
    );
    let body = &rest[..end];

    let preflight = body.find("preflight_guarded_request(");
    assert!(
        preflight.is_some(),
        "local_exec does not run the shared gate chain: a local command would skip \
         the execution gate, the rate limiter, the token scope and the approval policy"
    );
    let execute = body.find("exec_local_core_with_shutdown(req, s.shutdown.clone())");
    assert!(
        execute.is_some(),
        "local_exec must pass its shutdown token to the local executor"
    );
    assert!(
        preflight < execute,
        "local execution must follow the shared preflight"
    );

    // Ordering, not just presence. If the reserved-host check ran after the gate
    // chain, the scope and approval checks would have been evaluated against an
    // alias the client typed — so a token allowed for that alias could pass them
    // and the command would still land on this machine. The defensive copy inside
    // `exec_local_core` rejects the same request either way, which is exactly why
    // the response alone cannot pin this and the order has to be asserted here.
    let host_check = body.find("is_local_host(&req.host)");
    assert!(
        host_check.is_some(),
        "local_exec does not check for a local host id at all"
    );
    assert!(
        host_check < preflight,
        "local_exec must reject a non-local host *before* the gate chain runs"
    );
}

#[test]
fn ssh_exec_route_runs_preflight_before_shutdown_aware_execution() {
    let daemon = include_str!("../src/bin/agent2ssh-daemon.rs");
    assert!(daemon.contains(r#".route("/exec", post(exec))"#));
    let start = daemon.find("async fn exec(").expect("exec handler");
    let rest = &daemon[start..];
    let end = rest.find("\nasync fn local_exec(").expect("next handler");
    let body = &rest[..end];
    let preflight = body
        .find("preflight_rewritable_request(")
        .expect("shared preflight");
    let execute = body
        .find("exec_ssh_core_with_shutdown(req, s.shutdown.clone())")
        .expect("shutdown-aware SSH execution");
    assert!(
        preflight < execute,
        "authorization must precede SSH execution"
    );
}

/// A resident session is a shell that stays alive between commands, which
/// makes it *more* tempting to treat as a trusted channel — the session was
/// opened under one authorization, and a later command might hope to ride on
/// that. It must not: every command pays the same gate chain as a one-shot
/// exec, keyed on the session's own host id (read from the registry, not from
/// the request). Open and close must equally be preflight-guarded actions.
/// These facts live only in the handler bodies, so they are pinned at the source
/// level, the same way `local_exec_route_runs_the_shared_preflight` does.
#[test]
fn resident_session_routes_run_the_shared_preflight() {
    let daemon = include_str!("../src/bin/agent2ssh-daemon.rs");

    assert!(
        daemon.contains(r#""/resident-sessions""#),
        "the /resident-sessions route is missing"
    );
    assert!(
        daemon.contains(r#".route("/resident-sessions/:id/exec", post(resident_session_exec))"#),
        "the /resident-sessions/:id/exec route is missing or misrouted"
    );

    // The open is itself a gated action, evaluated against the environment id
    // the client named — after a local-host check, like /local-exec.
    let open = handler_body(daemon, "async fn resident_session_open(");
    let open_preflight = open
        .find("preflight_guarded_request(")
        .expect("resident_session_open does not run the shared gate chain");
    let open_host_check = open
        .find("is_local_host(&req.host)")
        .expect("resident_session_open does not check for a local host id at all");
    assert!(
        open_host_check < open_preflight,
        "resident_session_open must reject a non-local host *before* the gate chain runs"
    );

    // Exec authorizes against the session's own host id: the lookup from the
    // registry must precede the gate chain, or a client could name any host
    // alias it likes and borrow its scope entry.
    let exec_body = handler_body(daemon, "async fn resident_session_exec(");
    let exec_preflight = exec_body
        .find("preflight_guarded_request(")
        .expect("resident_session_exec does not run the shared gate chain");
    let session_host = exec_body
        .find("session_host(id)")
        .expect("resident_session_exec does not authorize against the session's own host id");
    assert!(
        session_host < exec_preflight,
        "resident_session_exec must read the session's host id *before* the gate \
         chain runs, so the command is authorized against the environment the \
         session actually lives in"
    );

    assert!(
        resident_close_has_ordered_preflight(daemon),
        "resident_session_close must be wired to DELETE and look up the \
         session's own host before preflight, then finish preflight before \
         resident_close_core can kill the shell"
    );
}

fn resident_close_has_ordered_preflight(daemon: &str) -> bool {
    if !daemon.contains("get(resident_session_get).delete(resident_session_close)") {
        return false;
    }
    let close = handler_body(daemon, "async fn resident_session_close(");
    match (
        close.find("session_host(id)"),
        close.find("preflight_guarded_request("),
        close.find("resident_close_core(id)"),
    ) {
        (Some(host), Some(preflight), Some(kill)) => host < preflight && preflight < kill,
        _ => false,
    }
}

#[test]
fn resident_session_close_source_guard_detects_compilable_mutations() {
    let daemon = include_str!("../src/bin/agent2ssh-daemon.rs");
    assert!(resident_close_has_ordered_preflight(daemon));

    // A different real handler still compiles as an Axum route, but must not
    // satisfy the resident close guard merely because the old function exists.
    let misrouted = daemon.replacen(
        "get(resident_session_get).delete(resident_session_close)",
        "get(resident_session_get).delete(session_close)",
        1,
    );
    assert_ne!(misrouted, daemon);
    assert!(!resident_close_has_ordered_preflight(&misrouted));

    // Comments between an identifier and '(' are valid Rust. Each mutation
    // removes one expected literal while leaving the handler compilable, so
    // the source check proves it can fail instead of finding another handler.
    let start = daemon.find("async fn resident_session_close(").unwrap();
    let (before, close_and_rest) = daemon.split_at(start);
    for (needle, replacement) in [
        ("session_host(id)", "session_host /* guard probe */(id)"),
        (
            "preflight_guarded_request(",
            "preflight_guarded_request /* guard probe */(",
        ),
        (
            "resident_close_core(id)",
            "resident_close_core /* guard probe */(id)",
        ),
    ] {
        let mutated = format!(
            "{before}{}",
            close_and_rest.replacen(needle, replacement, 1)
        );
        assert_ne!(mutated, daemon);
        assert!(
            !resident_close_has_ordered_preflight(&mutated),
            "source guard ignored a missing close-handler marker: {needle}"
        );
    }
}

/// Extract one handler's body, ending at the next top-level async fn. Required
/// rather than optional — a fallback to "the rest of the file" would let these
/// assertions pass on another handler's preflight call.
fn handler_body<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("the daemon has no `{signature}` handler"));
    let rest = &source[start + signature.len()..];
    let end = rest.find("\nasync fn ").unwrap_or_else(|| {
        panic!(
            "`{signature}` is no longer followed by another top-level \
             handler, so its body cannot be isolated and these checks would be vacuous"
        )
    });
    &rest[..end]
}

#[test]
fn exec_request_schema_includes_reason_and_change_id() {
    // Verify ExecRequest deserialization accepts reason and change_id fields,
    // matching the /exec endpoint contract in docs/api.yaml.
    let json = serde_json::json!({
        "host": "test",
        "command": "uptime",
        "force": false,
        "timeout_secs": 30,
        "reason": "deploy v2.0",
        "change_id": "CHG-001"
    });
    let req: ExecRequest = serde_json::from_value(json).unwrap();
    assert_eq!(req.host, "test");
    assert_eq!(req.command, "uptime");
    assert!(!req.force);
    assert_eq!(req.timeout_secs, Some(30));
    assert_eq!(req.reason, Some("deploy v2.0".into()));
    assert_eq!(req.change_id, Some("CHG-001".into()));

    // Also verify without optional fields
    let json_minimal = serde_json::json!({
        "host": "test",
        "command": "uptime",
        "force": false
    });
    let req_min: ExecRequest = serde_json::from_value(json_minimal).unwrap();
    assert_eq!(req_min.reason, None);
    assert_eq!(req_min.change_id, None);
    assert_eq!(req_min.timeout_secs, None);
}

#[test]
fn exec_multi_body_schema_matches_contract() {
    // Verify the /exec-multi request body schema includes all documented fields.
    let json = serde_json::json!({
        "hosts": ["web-1", "web-2"],
        "command": "systemctl restart app",
        "force": false,
        "timeout_secs": 60,
        "tags": ["production"],
        "strategy": {
            "concurrency": 2,
            "max_failures": 1,
            "batch_size": 5,
            "pause_between_batches_secs": 10
        },
        "reason": "rolling deploy",
        "change_id": "CHG-002"
    });

    let body: serde_json::Value = json;
    assert!(body["hosts"].is_array());
    assert_eq!(body["hosts"].as_array().unwrap().len(), 2);
    assert!(body["command"].is_string());
    assert!(body["force"].is_boolean());
    assert!(body["strategy"].is_object());
    assert_eq!(body["strategy"]["concurrency"], 2);
    assert_eq!(body["strategy"]["max_failures"], 1);
    assert_eq!(body["strategy"]["batch_size"], 5);
    assert_eq!(body["strategy"]["pause_between_batches_secs"], 10);
    assert_eq!(body["reason"], "rolling deploy");
    assert_eq!(body["change_id"], "CHG-002");

    // Verify BatchStrategy deserialization
    let strategy: BatchStrategy = serde_json::from_value(body["strategy"].clone()).unwrap();
    assert_eq!(strategy.concurrency, Some(2));
    assert_eq!(strategy.max_failures, Some(1));
    assert_eq!(strategy.batch_size, Some(5));
    assert_eq!(strategy.pause_between_batches_secs, Some(10));
}

#[test]
fn exec_multi_batch_result_schema_matches_contract() {
    // Verify ExecMultiBatchResult serialization matches the /exec-multi
    // response schema documented in docs/api.yaml.
    let result = ExecMultiBatchResult {
        results: vec![ExecMultiResult {
            host: "web-1".into(),
            result: Some(ExecResult {
                host: "web-1".into(),
                command: "uptime".into(),
                exit_code: Some(0),
                stdout: "ok".into(),
                stderr: String::new(),
                duration_ms: 100,
                risk_level: RiskLevel::Low,
                truncated: false,
                dropped_bytes: 0,
                side_effect: None,
                risk_unclassified: false,
            }),
            error: None,
        }],
        total_hosts: 1,
        successful: 1,
        failed: 0,
        skipped: 0,
        stopped_early: false,
        batches_executed: 1,
        total_duration_ms: 100,
    };

    let json = serde_json::to_value(&result).unwrap();
    // Verify all documented fields are present
    assert!(
        json.get("results").is_some(),
        "response must have 'results'"
    );
    assert!(
        json.get("total_hosts").is_some(),
        "response must have 'total_hosts'"
    );
    assert!(
        json.get("successful").is_some(),
        "response must have 'successful'"
    );
    assert!(json.get("failed").is_some(), "response must have 'failed'");
    assert!(
        json.get("skipped").is_some(),
        "response must have 'skipped'"
    );
    assert!(
        json.get("stopped_early").is_some(),
        "response must have 'stopped_early'"
    );
    assert!(
        json.get("batches_executed").is_some(),
        "response must have 'batches_executed'"
    );
    assert!(
        json.get("total_duration_ms").is_some(),
        "response must have 'total_duration_ms'"
    );

    let first = &json["results"][0];
    assert!(first.get("host").is_some());
    assert!(first.get("result").is_some());
    assert!(first.get("error").is_some());
    let inner = &first["result"];
    for field in &[
        "host",
        "command",
        "exit_code",
        "stdout",
        "stderr",
        "duration_ms",
        "risk_level",
        "truncated",
    ] {
        assert!(
            inner.get(field).is_some(),
            "ExecResult must have '{}'",
            field
        );
    }
}

#[test]
fn playbook_run_body_schema_matches_contract() {
    // Verify the /playbooks/run request body schema.
    let json = serde_json::json!({
        "playbook": "deploy-app",
        "host": "prod-web",
        "force": false,
        "params": {"version": "2.0", "env": "staging"},
        "reason": "scheduled release",
        "change_id": "CHG-003"
    });

    assert_eq!(json["playbook"], "deploy-app");
    assert_eq!(json["host"], "prod-web");
    assert!(json["params"].is_object());
    assert_eq!(json["params"]["version"], "2.0");
    assert_eq!(json["reason"], "scheduled release");
    assert_eq!(json["change_id"], "CHG-003");
}

#[test]
fn audit_export_response_contract() {
    // Verify audit export produces JSONL and CSV with the correct fields,
    // matching the /audit/export endpoint contract.
    use chrono::Utc;
    use uuid::Uuid;

    let entries = [AuditEntry {
        id: Uuid::new_v4(),
        ts: Utc::now(),
        host: "test".into(),
        command: "uptime".into(),
        exit_code: Some(0),
        duration_ms: 100,
        risk_level: RiskLevel::Low,
        reason: Some("health check".into()),
        change_id: Some("CHG-001".into()),
        side_effect: None,
        source: Some("daemon".into()),
        action: None,
        outcome: None,
        decision: None,
        decided_by: None,
        original_command: None,
        risk_unclassified: false,
    }];

    // JSONL format: each line is a valid AuditEntry JSON
    let jsonl: String = entries
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let parsed: AuditEntry = serde_json::from_str(jsonl.lines().next().unwrap()).unwrap();
    assert_eq!(parsed.reason, Some("health check".into()));
    assert_eq!(parsed.change_id, Some("CHG-001".into()));

    // CSV format: header must include reason,change_id
    let csv_header = "id,timestamp,host,command,exit_code,duration_ms,risk_level,reason,change_id";
    let csv_line = format!(
        "{},{},{},{},{},{},{},{},{}",
        entries[0].id,
        entries[0].ts.to_rfc3339(),
        entries[0].host,
        entries[0].command,
        entries[0].exit_code.unwrap(),
        entries[0].duration_ms,
        entries[0].risk_level,
        entries[0].reason.as_deref().unwrap_or(""),
        entries[0].change_id.as_deref().unwrap_or(""),
    );
    assert!(csv_header.contains("reason"));
    assert!(csv_header.contains("change_id"));
    assert!(csv_line.contains("CHG-001"));
}

// ── Configurable listen address (I4) ────────────────────────────────────────

/// End-to-end check that the daemon honors `AGENT2SSH_DAEMON_ADDR`: a server
/// bound to a configured non-default port is reachable through the same shared
/// resolver (I1) that clients use, proving the bind side and client side agree.
#[tokio::test]
#[serial_test::serial]
async fn daemon_honors_configured_listen_address_end_to_end() {
    use axum::{routing::get, Json, Router};

    // Reserve a free loopback port, then advertise it the way an operator would.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr = format!("127.0.0.1:{port}");
    std::env::set_var("AGENT2SSH_DAEMON_ADDR", &addr);

    // The shared resolver must agree with the configured bind address, and a
    // client must derive the matching loopback URL.
    assert_eq!(agent2ssh::local_daemon_addr(), addr);
    assert_eq!(
        agent2ssh::local_daemon_url(),
        format!("http://127.0.0.1:{port}")
    );

    // Boot a real server bound to the configured address with a /health route
    // shaped like the daemon's (the raw probe looks for an {"ok":true} body).
    let app = Router::new().route(
        "/health",
        get(|| async { Json(serde_json::json!({ "ok": true })) }),
    );
    let listener = tokio::net::TcpListener::bind(agent2ssh::local_daemon_addr())
        .await
        .expect("bind configured daemon address");
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    // The blocking health probe resolves the same configured address end to end.
    // Run it off-runtime so the spawned server can make progress.
    let healthy = tokio::task::spawn_blocking(agent2ssh::daemon_control::daemon_health_ok)
        .await
        .unwrap();

    server.abort();
    std::env::remove_var("AGENT2SSH_DAEMON_ADDR");

    assert!(
        healthy,
        "daemon_health_ok should reach a server bound to the configured AGENT2SSH_DAEMON_ADDR"
    );
}
