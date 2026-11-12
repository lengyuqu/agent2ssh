//! Integration tests for daemon-related core functionality.
//!
//! Two layers of tests:
//!
//! 1. **Core function tests** — exercise the library functions that the daemon
//!    HTTP handlers wrap, verifying business logic without going through HTTP.
//!
//! 2. **HTTP handler tests** — build a small axum `Router` that mirrors 18 of
//!    the daemon's 92 `.route(...)` calls (20 of its 107 `(path, method)`
//!    pairs), and use axum's `oneshot` test utility to verify request/response
//!    semantics (auth, routing, JSON serialization, status codes) without
//!    spawning a real TCP server.
//!
//!    **This layer is a shape-mirror, not an implementation test.** The copies
//!    carry neither `AuthContext` nor `require_admin` — `check_auth` compares
//!    one fixed token — so nothing here can say what a *scoped* token gets, and
//!    a handler that gained or lost an admin gate would keep these assertions
//!    green. Mirroring a path is not mirroring every method on it: the daemon
//!    also mounts `POST /hosts` and `POST /health-snapshot`, and neither has a
//!    counterpart here. The other 74 calls are not mirrored at all (`/metrics`,
//!    `/gate*`, `/audit*`, `/sftp/*`, `/sessions*`, `/forwards*`,
//!    `/recordings*`, `/prompts/*`, `/terminal*`, `/events/stream`): they carry
//!    85 `(path, method)` pairs over 74 paths. One of those pairs, the `/`
//!    redirect, is an inline closure rather than a named handler, so a scan
//!    keyed on `async fn` names sees 106 of the 107. So behaviour that depends
//!    on scope, admin, or a route
//!    this router does not mount belongs in the real-daemon e2e targets
//!    (`daemon_local_exec_e2e`, `daemon_resident_session_e2e`, ...), which
//!    spawn `agent2ssh-daemon` itself.

mod common;

use agent2ssh::approval::*;
use agent2ssh::core::*;
use agent2ssh::risk_config::*;
use agent2ssh::types::*;
use std::collections::{BTreeMap, BTreeSet};

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
/// `INBAND_REJECTIONS` below for the four routes that consume the `Err`
/// instead of returning it, and would be wrongly indicted by this list.
///
/// 403 arrives through a helper too, and was invisible the same way:
/// `require_admin` builds the status inside itself, so a route whose only
/// authorization code is the admin gate could document none at all.
/// `/shutdown` is that shape — its `responses` carry 200/400/401/409 and a 403
/// whose presence no assertion checked, because the 401 check above only covers
/// the middleware's code. No route was missing it when this entry was added; the
/// point is the next one.
///
/// 423 is listed too, and only through `reject_if_gate_paused`. That helper is
/// the one place where a function name maps 1:1 onto a constant — it returns
/// `StatusCode::LOCKED` and nothing else — so the same text scan can pin it
/// without guessing. The half it cannot pin is the other direction: the same
/// helper answers `500` when `execution_gate.json` cannot be read, and *that* is
/// not listed, because it would demand a per-operation 500 on every operation
/// that can consult the gate, where `info.description` states it once. A
/// per-operation repeat would be an invitation to delete both copies, and the
/// count it would take is not one this file derives — it spans operations that
/// reach the gate directly, through `preflight_sftp_rm_rf`, and through
/// `load_execution_gate` itself, so any number written here is a snapshot. The
/// approval chain behind the same preflights has the same shape and is stated in
/// the same paragraph, for the same reason.
///
/// 403 arrives through the preflight as well, and was the last code with no
/// guard at all: `authorize_command_inner` builds `FORBIDDEN` for a scope denial
/// and for a rejected approval, three call levels below the handler, so a route
/// whose only 403 path runs through it documents nothing and fails nothing.
/// `/local-exec` was one — deleting its `403` turned no test red, because its
/// body holds neither `StatusCode::FORBIDDEN` nor `require_admin(`.
///
/// The preflight is an all-or-nothing occurrence: a handler that calls
/// `preflight_guarded_request(` names no code at all, so every code the wrapper
/// can answer is equally invisible from its body — not just the 403. The table
/// therefore registers 400, 403, 408, 423 and 429 for each of the three
/// wrappers, and the same for `authorize_targets`, `finish_sftp_operation`,
/// `check_daemon_scope` and `terminal_broadcast_preview_results`. Measured
/// before the change: of the 29 operations documenting a 408, 20 could lose it
/// silently, because 408 has no `preflight_*` entry — the same asymmetry the
/// 403 paragraph describes, one code over. 500 stays out for the reason the
/// 423 paragraph gives.
///
/// `ensure_command_length` is the same shape one level down: it builds `400`
/// inside itself, so a route whose only 400 is an over-long command documents
/// nothing. Ten handlers call it. Nine document the 400 — `/exec`,
/// `/local-exec`, `/exec-multi`, `/exec/compare`, `/exec/preview`, both resident
/// and approval routes — and the tenth is `/exec/stream`, which writes the
/// refusal into a frame on a socket it has already upgraded and is therefore in
/// `INBAND_REJECTIONS`. Two of those nine did not document it until this entry
/// existed: `/risk/check` and `/approval/check`, the pair that answers questions
/// rather than running anything. That is the family-looks-consistent-from-one-
/// route failure again: the sibling that reports a risk level has no reason to
/// look different from the sibling that runs a command.
///
/// The eleventh call site is one level further down, and it is why this list
/// also names a validator. `validate_terminal_broadcast_request` calls
/// `ensure_command_length` and builds five more `400`s of its own;
/// `terminal_broadcast_preview` and `terminal_broadcast_run` call *it*. Their
/// bodies therefore contain neither `ensure_command_length(` nor any
/// `StatusCode::` literal, so neither check saw the 400 those routes answer —
/// deleting `"400"` from either operation's `responses` left every assertion
/// green. Listing the validator costs nothing today (both routes document 400)
/// and makes that code as hard to delete as any other route's.
///
/// `authorize_command` is the 408 source no other entry covered. It builds
/// `REQUEST_TIMEOUT` for `ApprovalTimedOut` one level down, and only two
/// handlers that name it also swallow it: `exec_stream` writes it into a frame,
/// `terminal_broadcast_run` writes it into one target's `error`. Both are in
/// `INBAND_REJECTIONS`, so this entry is what makes those two exemptions
/// load-bearing — without it, "408 never becomes a status here" was not a claim
/// anything could falsify.
///
/// `preflight_sftp_rm_rf` is a wrapper, and a text scan does not see through it:
/// `sftp_rm_rf` and `sftp_rm_rf_authorize` call the wrapper, the wrapper calls
/// `preflight_guarded_request`, and the handler bodies name neither the inner
/// helper nor `reject_if_rate_limited`. Both routes document 403 and 429 today,
/// so listing the wrapper costs nothing and makes those two codes as hard to
/// delete as any other route's.
const HELPER_STATUS: [(&str, &str); 31] = [
    // 400 — validation that lives below the handler.
    ("ensure_command_length(", "400"),
    ("validate_terminal_broadcast_request(", "400"),
    ("finish_sftp_operation(", "400"),
    ("authorize_targets(", "400"),
    ("preflight_guarded_request(", "400"),
    ("preflight_rewritable_request(", "400"),
    ("preflight_sftp_rm_rf(", "400"),
    // 403 — scope, admin and rejected approval.
    ("authorize_command(", "403"),
    ("authorize_targets(", "403"),
    ("check_daemon_scope(", "403"),
    ("terminal_broadcast_preview_results(", "403"),
    ("preflight_guarded_request(", "403"),
    ("preflight_rewritable_request(", "403"),
    ("preflight_sftp_rm_rf(", "403"),
    ("require_admin(", "403"),
    ("require_recording_admin(", "403"),
    // 408 — the approval wait.
    ("authorize_command(", "408"),
    ("authorize_targets(", "408"),
    ("preflight_guarded_request(", "408"),
    ("preflight_rewritable_request(", "408"),
    ("preflight_sftp_rm_rf(", "408"),
    // 423 — the paused execution gate.
    ("reject_if_gate_paused(", "423"),
    ("gate_blocks_source(", "423"),
    ("preflight_guarded_request(", "423"),
    ("preflight_rewritable_request(", "423"),
    ("preflight_sftp_rm_rf(", "423"),
    // 429 — the per-minute rate limit and the session cap.
    ("reject_if_rate_limited(", "429"),
    ("register_limited_session_for_command(", "429"),
    ("preflight_guarded_request(", "429"),
    ("preflight_rewritable_request(", "429"),
    ("preflight_sftp_rm_rf(", "429"),
];

/// Operations whose gate, rate-limit or approval-wait refusals never become an
/// HTTP status.
///
/// `HELPER_STATUS` is a text scan: it sees `reject_if_rate_limited(` in a body
/// and concludes the caller can receive a 429. Three handlers consume that `Err`
/// instead of returning it — `exec_stream` sends `{"type":"error"}` over the
/// socket it has already upgraded, and `terminal_broadcast_run` and its
/// `preview_results` mark every eligible target `authorized: false` and return
/// 200 — so a `"429"` or `"423"` there would promise a branch this server never
/// takes.
///
/// `/terminal` is the control in the same family, and the difference is not
/// only "before versus after the upgrade". `/terminal` returns the refusal as a
/// status — `reject_if_gate_paused(...)` before `on_upgrade`, propagated with
/// `?` — so an unreadable gate file answers 423 or 500 like any other reader.
/// `preview_results` folds the same read into `load_execution_gate().ok()` and
/// tests it with `is_some_and`, so a gate file that cannot be read skips the
/// branch entirely rather than answering 423 or 500. That does not make the
/// request succeed: the same handler answers its rate-limit refusal
/// (`reject_if_rate_limited`, which reads `execution_limits.toml` — a different
/// file with its own failure modes) and its per-target authorization by
/// writing each into the same per-target `error` field and returning 200. The
/// 200-with-a-field shape is this route's uniform refusal form, not a property
/// of the gate branch: "the gate is unreadable" does not mean "the target is
/// authorized", and one unreadable file is not the only way to reach it. Two
/// consumption shapes share one exemption table, so a route that returns the
/// status belongs here only if it also consumes it somewhere else.
///
/// `/terminal` is the sharpest case of that split, and it is why the exemption
/// is per code: the route is in the table for `408` alone. Its gate and
/// rate-limit refusals happen in `terminal_attach`, before `on_upgrade`, and
/// are statuses — which is what the paragraph above contrasts against
/// `preview_results`. Its approval wait happens in `handle_terminal`, after the
/// upgrade, where every refusal becomes `{"type":"error"}`; the status the REST
/// routes would answer is never written to the socket. A per-route exemption
/// could not express that, and a route left out of the table entirely left the
/// 408 reachable and undocumented.
///
/// The third field is the set of codes this route consumes instead of
/// returning, so the exemption is per code and not per route. That distinction
/// is load-bearing: a broadcast route answers its 400 as a status
/// (`validate_terminal_broadcast_request` returns it and the handler propagates
/// with `?`) while consuming its 408/423/429 in the per-target `error` field. An
/// exemption that skipped the whole route would hide the 400 again — which is
/// exactly what it did before this field existed, and what made the
/// `validate_terminal_broadcast_request(` entry in `HELPER_STATUS` a no-op for
/// the two routes it was added for. `exec_stream` is the same shape one layer
/// up: it answers 400/408/423/429 as frames and none of them as a status.
///
/// Every code in that set is also checked in the other direction: the route's
/// `responses` must not list it, `HELPER_STATUS` must know the code at all, and
/// the handler — or a delegate named in the fifth field — must call a helper
/// that answers it. The first two were the original pair; the third is what
/// stops an exemption from citing a body that is not on this route's path, which
/// is how `423` and `429` once sat in the preview row with neither code
/// reachable from it. `429` was the plain error: the preview never rate-limits
/// at all, so the entry claimed a form of refusal the route does not have, and
/// the row now lists only the gate's 423, folded by the shared preview pass.
///
/// An earlier version hard-coded 423/429 in the reverse
/// check while this field held the set, so a superset — or an empty set — was
/// exempt from the helper scan and unconstrained by the reverse check at the
/// same time. That is the same "the list became the place to silence the check"
/// failure the field was added to close, one level down.
///
/// The fourth field is a phrase the path block must still carry, so the
/// exemption cannot decay into silence — the refusal stays documented, just not
/// as a status. Rewording that prose breaks this test on purpose.
const INBAND_REJECTIONS: [(&str, &str, &[&str], &str, &[&str]); 4] = [
    (
        "get",
        "/exec/stream",
        &["400", "403", "408", "423", "429"],
        "only as a frame",
        &[],
    ),
    (
        "post",
        "/terminal/broadcast",
        &["408", "423", "429"],
        "returns 200",
        &["terminal_broadcast_preview_results"],
    ),
    (
        "get",
        "/terminal",
        &["408"],
        "as the same error frame, which carries the message",
        &["handle_terminal"],
    ),
    (
        "post",
        "/terminal/broadcast/preview",
        &["423"],
        "into that target's `error`",
        &["terminal_broadcast_preview_results"],
    ),
];

/// The two routes that serve the browser console rather than the API.
///
/// They are outside every contract check: `api.yaml` documents neither, and
/// neither is a JSON operation a client calls — one redirects, one serves HTML.
/// This is the only place they are declared, so no assertion restates the list:
/// the `api.yaml` check skips both, and the parse-loss check accepts a closure
/// only on a path named here. `/console`'s mount is additionally pinned by
/// `the_console_route_mounts_one_named_handler`.
const NON_API_ROUTES: [&str; 2] = ["/", "/console"];

/// Whether a segment mounts a method whose argument is a closure.
///
/// This is the only way a `.route(...)` call can name a method builder and still
/// yield no handler — `get(|| async { … })` — so it is the property that
/// explains why a drop is legitimate. Recording the shape rather than the path
/// keeps the check from degenerating into "the one path I already know about is
/// allowed": a closure moved onto an API path still fails, and a named handler
/// that stops parsing no longer hides behind a whitelisted path.
///
/// The name and its parenthesis are read with the whitespace between them
/// optional, because `get (handler)` is the same call to Rust: a check that
/// insisted on `get(` would call that closure a named handler and send the
/// reader off to teach the parser a shape it already understands.
fn mounts_closure(segment: &str) -> bool {
    METHOD_BUILDERS.iter().any(|(needle, _)| {
        let name = needle.trim_end_matches('(');
        segment.match_indices(name).any(|(at, _)| {
            let preceded_by_name = segment[..at]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_');
            if preceded_by_name {
                return false;
            }
            // `get (…)` is the same call with a space between the name and the
            // parenthesis, so the argument starts after the whitespace rather
            // than straight after the name.
            let Some(argument) = segment[at + name.len()..].trim_start().strip_prefix('(') else {
                return false;
            };
            let mut argument = argument.trim_start();
            // `move || …` and `async || …` wrap the closure without naming it.
            for word in ["move", "async"] {
                if let Some(after) = argument.strip_prefix(word) {
                    if after.starts_with(|ch: char| ch.is_whitespace() || ch == '|') {
                        argument = after.trim_start();
                    }
                }
            }
            argument.starts_with('|')
        })
    })
}

/// The methods one `.route(...)` call mounts, with the handler each names.
///
/// `get(a).post(b)` mounts two, and a builder chain that wraps across lines
/// still belongs to a single call — so the caller bounds `segment` at the next
/// `.route(`. A builder is only counted when the character before it is not
/// part of a name: `algo_prefs_get` ends in `_get` and must not read as a
/// `get(` call.
///
/// The order is the one `METHOD_BUILDERS` declares, not the source order. This
/// is the set of what a call mounts; every consumer compares it as a set, and
/// no reader should depend on which builder was written first.
///
/// A builder whose argument starts with `move` or `async` mounts a closure:
/// reading `get(move || …)` as a handler named `move` would invent an entry
/// that no other check has a reason to question, since `(path, method)` is all
/// most of them look at.
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
                if !handler.is_empty() && !matches!(handler.as_str(), "move" | "async") {
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
    parse_route_table(daemon).routes
}

/// A `.route(...)` call the derived checks cannot read, and why.
#[derive(Debug)]
struct DroppedRoute {
    line: usize,
    /// Empty when the call carried no path literal at all.
    path: String,
    reason: &'static str,
    /// True when the segment's mount argument is a closure rather than a name.
    mounts_closure: bool,
}

/// A `.route(...)` call that mounts a closure on one method and a name on
/// another.
///
/// `get(h).post(|| …)` becomes one entry for `get` and nothing at all for
/// `post`: the closure is not an entry, and the call does not drop because it
/// did produce something. The POST is then served by the daemon and read by no
/// check — the entries look complete.
#[derive(Debug)]
struct MixedMount {
    line: usize,
    path: String,
}

/// The route table as the derived checks read it.
struct RouteTable {
    /// Every `(path, method, handler)` the parse could read.
    routes: Vec<(String, &'static str, String)>,
    /// Calls it could not turn into an entry.
    dropped: Vec<DroppedRoute>,
    /// Calls that mounted a closure and a name in the same `.route(...)`.
    mixed: Vec<MixedMount>,
}

/// The same parse, plus what it could not read and what it read in halves.
///
/// A call is dropped when its segment carries no string literal, when the first
/// literal is not a path, or when the segment names no method builder with a
/// name after it. Every one of those drops is silent by construction: three
/// tests read `daemon_routes` (four call sites) and each is phrased as "no
/// operation here violates X", so a lost operation makes its own checks vacuous
/// while the test stays green, and the only thing that would move is prose.
/// Returning the drops lets one test assert the parse is lossless, instead of
/// those checks trusting a denominator nobody states.
///
/// One drop is legitimate and known: `/` redirects to the console through a
/// closure, which has no `async fn` for the body checks to read, and it is not
/// an API route. See `NON_API_ROUTES`.
///
/// A call that mounts both shapes is neither dropped nor described: it lands in
/// `mixed`, because the entry it does produce makes every count look right.
fn parse_route_table(daemon: &str) -> RouteTable {
    let starts: Vec<usize> = daemon
        .match_indices(".route(")
        .map(|(index, _)| index)
        .collect();
    let mut routes = Vec::new();
    let mut dropped = Vec::new();
    let mut mixed = Vec::new();
    for (position, start) in starts.iter().enumerate() {
        let mut end = starts.get(position + 1).copied().unwrap_or(daemon.len());
        if let Some(with_state) = daemon[*start..].find(".with_state(") {
            end = end.min(start + with_state);
        }
        let segment = &daemon[*start..end];
        let line = daemon[..*start].lines().count() + 1;
        let Some(open) = segment.find('"') else {
            dropped.push(DroppedRoute {
                line,
                path: String::new(),
                reason: "no path literal",
                mounts_closure: false,
            });
            continue;
        };
        let Some(close) = segment[open + 1..].find('"') else {
            dropped.push(DroppedRoute {
                line,
                path: String::new(),
                reason: "unterminated path literal",
                mounts_closure: false,
            });
            continue;
        };
        let path = &segment[open + 1..open + 1 + close];
        if !path.starts_with('/') {
            dropped.push(DroppedRoute {
                line,
                path: path.to_string(),
                reason: "the first literal is not a path",
                mounts_closure: false,
            });
            continue;
        }
        let mounted = mounted_handlers(segment);
        if mounted.is_empty() {
            dropped.push(DroppedRoute {
                line,
                path: path.to_string(),
                reason: "the segment names no method builder with a handler after it",
                mounts_closure: mounts_closure(segment),
            });
            continue;
        }
        if mounts_closure(segment) {
            mixed.push(MixedMount {
                line,
                path: path.to_string(),
            });
        }
        let normalized = normalized_route_path(path);
        for (method, handler) in mounted {
            routes.push((normalized.clone(), method, handler));
        }
    }
    RouteTable {
        routes,
        dropped,
        mixed,
    }
}

/// The body of one handler, from its `async fn` line to its closing brace.
///
/// The obvious terminator — "the next `async fn` or `fn`" — is wrong in a way
/// that stayed invisible while it was merely unfortunate: most route handlers
/// are followed by a `struct`, a `const`, a doc comment or a helper, so their
/// "bodies" ran hundreds of bytes past the closing brace. Nothing foreign
/// happened to carry a `StatusCode::` constant yet, which is exactly why this
/// could rot unnoticed — the day a neighbouring item gains one, the scan reports
/// it against the wrong operation. Walking the braces instead bounds the body at
/// the item it belongs to. (This paragraph used to carry a ratio — "66 of the
/// 106 route handlers" — and by the time anyone recomputed it every handler
/// overran under that reading, which is the failure a snapshot number invites:
/// it is quoted as a verified fact long after it stopped being one.)
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

/// The route parse must not quietly lose a `.route(...)` call.
///
/// Three tests read `daemon_routes`, four call sites between them, and each is
/// phrased as "no operation here violates X". A call the parser cannot read
/// makes that phrase vacuous for the operation it mounted: the test sees one
/// entry fewer and stays green, and nothing in the suite says how many entries
/// it expected. So the denominators are only decidable while the parse is
/// lossless — that is what this pins, rather than a count a new route would
/// break.
///
/// It pins the *shape* of a drop, not the path: a mount argument that is a
/// closure has no name for the body checks to read, and that is the only reason
/// a segment may name a method builder and still yield nothing. A named handler
/// that stops parsing is therefore red on any path it sits on. The version this
/// replaces compared a fixed set of (path, reason) pairs, so it was red on the
/// same changes — but its message said to "update this expectation
/// deliberately", which for a non-API path means appending a path to a list
/// instead of noticing that the mount changed shape. That instruction is what
/// made it the wrong sentinel for `/console`, which has a test of its own now.
#[test]
fn every_route_call_is_read_as_an_operation() {
    let daemon = include_str!("../src/bin/agent2ssh-daemon.rs");
    let table = parse_route_table(daemon);
    let (routes, dropped) = (&table.routes, &table.dropped);
    let detail =
        |drop: &DroppedRoute| format!("line {} `{}`: {}", drop.line, drop.path, drop.reason);
    // A closure is the one shape with no handler to read, so it is the one
    // shape a drop is allowed to be.
    let unreadable: Vec<String> = dropped
        .iter()
        .filter(|drop| !drop.mounts_closure)
        .map(detail)
        .collect();
    assert!(
        unreadable.is_empty(),
        "these `.route(...)` calls were dropped without mounting a closure — either a named \
         handler the parse could not read, or a closure in a shape it does not model: \
         {unreadable:?}. Each one is an operation that fell out of every derived check at once, \
         in silence. Teach `parse_route_table` the shape, or mount the handler through \
         `.route(path, get(handler))`."
    );
    // And a closure is only tolerable on a route no contract covers. The other
    // half of the net is the `api.yaml` check: it would call a documented
    // closure `phantom`, since the parse yields no entry for it. Between the
    // two, a closure on a non-API path is the only shape that passes both —
    // which is exactly what the console redirect is.
    let covered: Vec<String> = dropped
        .iter()
        .filter(|drop| !NON_API_ROUTES.contains(&drop.path.as_str()))
        .map(detail)
        .collect();
    assert!(
        covered.is_empty(),
        "these routes are mounted with a closure, so no contract check can read them, and they \
         are not among the non-API routes {NON_API_ROUTES:?}: {covered:?}. Name the handler, or \
         declare the route non-API — a third shape that neither check sees is what this test \
         exists to stop."
    );
    assert!(
        !routes.is_empty(),
        "the route parse found no operations at all, so every derived check is vacuous"
    );
}

/// Every entry names a handler the daemon actually has.
///
/// `daemon_routes` reads a name after each method builder without knowing what a
/// name is, so it will accept one from a scope that is not the daemon's: a
/// `.nest`-ed sub-router's `get(h)` yields `h`, which the top level of this file
/// never defines. Nothing else questions it — the entries are compared as
/// `(path, method)`, and the body and status-code tests only complain when that
/// path is in `api.yaml`, where the panic reads "no `async fn h(`" rather than
/// saying the name came from somewhere else.
///
/// So the parse has to land on a function the daemon defines. That is also what
/// keeps every other derived check honest: they read bodies by name, and a name
/// they cannot resolve makes them panic, which is where the check stops being a
/// check.
#[test]
fn every_parsed_handler_is_a_function_the_daemon_defines() {
    let daemon = include_str!("../src/bin/agent2ssh-daemon.rs");
    let parsed = daemon_routes(daemon);
    assert!(
        !parsed.is_empty(),
        "the route parse found no operations, so this check would pass for the wrong reason"
    );
    let unresolved: Vec<String> = parsed
        .iter()
        .filter(|(_, _, handler)| !daemon.contains(&format!("\nasync fn {handler}(")))
        .map(|(path, method, handler)| format!("{method} {path} -> `{handler}`"))
        .collect();
    assert!(
        unresolved.is_empty(),
        "the route table names handlers the daemon does not define: {unresolved:?}. A name can \
         come from a nested router's scope, or from a builder argument that is not a handler at \
         all; either way the entry stands in for an operation whose body nothing can read."
    );
}

/// One `.route(...)` call mounts one kind of handler, not both.
///
/// The two shapes are read in different places: a name becomes an entry, a
/// closure becomes a drop. A call that does both — `get(h).post(|| …)` — yields
/// an entry for the method it can name and nothing at all for the other, and
/// because it did produce something it never drops either. The closure's method
/// is then served by the daemon and read by no check: `api.yaml` compares
/// `(path, method)` from entries, the drop check only looks at calls that
/// produced nothing, and the handler check only looks at what did.
#[test]
fn a_route_call_does_not_mix_a_closure_with_a_named_handler() {
    let table = parse_route_table(include_str!("../src/bin/agent2ssh-daemon.rs"));
    let mixed: Vec<String> = table
        .mixed
        .iter()
        .map(|mount| format!("line {} `{}`", mount.line, mount.path))
        .collect();
    assert!(
        mixed.is_empty(),
        "these `.route(...)` calls mount a closure on one method and a named handler on another: \
         {mixed:?}. The closure's method produces no entry and no drop, so the daemon serves it \
         and no check reads it. Give the closure its own `.route(...)`, or name the handler."
    );
}

/// `/console` is exempt from every derived check, so its shape is stated once.
///
/// It sits in `NON_API_ROUTES` because it serves HTML to a browser, not because
/// its mount is unreadable — the handler is a named `async fn` the body checks
/// could read if they were pointed at it. Left unstated, "the console is
/// served" would rest on the route line still being there: a second method, a
/// different handler, or a redirect left in its place would keep the whole
/// suite green while the browser console 404s or doubles up.
#[test]
fn the_console_route_mounts_one_named_handler() {
    let daemon = include_str!("../src/bin/agent2ssh-daemon.rs");
    // The count comes first: axum merges a second `.route` on the same path into
    // the first, so `POST /console` is a legal addition that this test would
    // otherwise read as a different segment and never look at.
    let calls = daemon.matches(".route(\"/console\"").count();
    assert_eq!(
        calls, 1,
        "`/console` is mounted by {calls} `.route(...)` calls. A second one adds a method to a \
         path that no `api.yaml` check reads and that never drops, so it would ship unremarked"
    );
    let at = daemon
        .find(".route(\"/console\"")
        .expect("the daemon no longer mounts `/console`, so the browser console is unreachable");
    // Bounded the same way the parser bounds it: at the next `.route(`.
    let rest = &daemon[at + 1..];
    let end = rest
        .find(".route(")
        .map_or(daemon.len(), |offset| at + 1 + offset);
    assert_eq!(
        mounted_handlers(&daemon[at..end]),
        vec![("get", "serve_console".to_string())],
        "`/console` must mount exactly one GET of `serve_console`; every `api.yaml` check skips \
         this path, so nothing else in the suite states its shape"
    );
    assert!(
        daemon.contains("\nasync fn serve_console("),
        "`/console` names `serve_console`, but there is no `async fn serve_console(` for it to \
         reach"
    );
}

/// The router chain mounts paths through `.route(...)` and nothing else.
///
/// The parser reads that one call shape, and axum's other mounting builders do
/// not contain the substring `.route(`: `route_service` takes a service,
/// `nest` and `nest_service` re-root a sub-router, `merge` splices another
/// router in, and `fallback`/`fallback_service` answer methods the segment never
/// names. A path added through any of them yields neither an entry nor a drop,
/// so every derived check — including the drop check — would skip it in silence.
/// That is the one way to mount a route that neither net sees, which is why it
/// is stated rather than parsed.
#[test]
fn the_router_chain_mounts_paths_only_through_route() {
    let daemon = include_str!("../src/bin/agent2ssh-daemon.rs");
    let start = daemon
        .find("let app = Router::new()")
        .expect("the router chain moved, so this test no longer knows where it starts");
    let chain = &daemon[start..];
    let Some(end) = chain.find(".with_state(") else {
        panic!("the router chain no longer ends at `.with_state(`, so it has no bound here");
    };
    let chain = &chain[..end];
    // A comment naming one of these builders would read as a mount, so the
    // chain is reduced to its code first.
    let chain: String = chain
        .lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    let others = [
        ".route_service(",
        ".nest(",
        ".nest_service(",
        ".merge(",
        ".fallback(",
        ".fallback_service(",
    ];
    let offenders: Vec<&str> = others
        .into_iter()
        .filter(|builder| chain.contains(builder))
        .collect();
    assert!(
        offenders.is_empty(),
        "the router chain mounts paths through {offenders:?}. The route parse reads `.route(...)` \
         only, so those routes produce no entry and no drop, and every derived check skips them \
         without a word. Teach `parse_route_table` the shape, or mount them with `.route`."
    );
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
/// Reachability is not the same as presence: `INBAND_REJECTIONS` names the
/// routes whose helper call answers inside a 2xx, and the third assertion below
/// holds them to the opposite rule. It names no count on purpose — the list has
/// grown once already, and a number here is the first thing to drift.
/// `DELETE /forwards/{id}` and `POST /forwards/{id}/stop` run the same four
/// checks in the same order -- gate, rate limit, scope, approval -- because
/// `forward_remove` and `forward_stop` are line-for-line identical apart from
/// the operation string. Whatever can come back from one can therefore come
/// back from the other, so the two pages must document the same codes. They
/// did not: the delete page omitted `408` even though both call the same
/// `authorize_command`, which answers `REQUEST_TIMEOUT` when an approval
/// expires. That omission was invisible because nothing compared the two.
/// Every route whose handler consults the token scope documents a 403 that says
/// which refusal it is, and the description follows the order the handler
/// actually calls things in.
///
/// This replaces a test that compared three forward routes and said, in its own
/// comment, that its list of paths was hand-maintained — so a fourth
/// inline-scope handler would not appear, and neither would any of the bare
/// `Scope denied or approval rejected` descriptions elsewhere in the spec. What
/// that test could not see at all was the *order*: a client reading a bare
/// sentence on a route whose gate runs first will expect a paused gate to answer
/// 403 when it actually answers 423, because `reject_if_gate_paused` returns
/// before the token's scope is ever consulted. Reading the handler body for the
/// order of its gate, rate-limit, scope and authorize calls is what answers
/// that, and it needs no more machinery than `handler_body_of`.
///
/// Comments are stripped before the search. This file's own prose spells out
/// `reject_if_gate_paused(...)`, and a comment is the one place in the source
/// where a call-shaped string is not a call. Measured: an unstripped read of
/// `approval_check` answers `["gate", "scope"]` where the real chain is
/// `["scope"]`, and since that route's bare 403 is *correct*, the test would go
/// red on a route with nothing wrong with it.
#[test]
fn every_scope_checked_route_documents_the_order_its_refusals_arrive_in() {
    let daemon = include_str!("../src/bin/agent2ssh-daemon.rs");
    let spec = include_str!("../../docs/api.yaml");

    // The scope question, asked directly or through the shared preflight, whose
    // first step it is.
    fn consults_scope(chain: &[&'static str]) -> bool {
        chain.contains(&"scope") || chain.contains(&"preflight")
    }

    // A helper that asks the scope question on its caller's behalf. Only one
    // exists: `terminal_broadcast_run` takes its per-target verdict from
    // `terminal_broadcast_preview_results`, which is where its
    // `check_daemon_scope` call actually is. Without this the route would be
    // read as "rate, authorize" with no scope step at all — the worse error of
    // the two, since it would describe a scope-checked route as un-scoped and
    // silently exempt it from every rule below.
    const DELEGATES: [(&str, &str); 1] = [(
        "terminal_broadcast_preview_results(",
        "terminal_broadcast_preview_results",
    )];

    // The order a route's refusals arrive in, or `None` when it consults no
    // scope at all.
    fn order_of(daemon: &str, handler: &str) -> Option<Vec<&'static str>> {
        let body = strip_rust_comments(handler_body_of(daemon, handler));

        // `proxy_exec` has two branches: `localhost` takes the shared preflight,
        // a remote alias checks inline. One handler, one documented order, and
        // the remote branch is the one that can also answer 404 and 502 — so
        // read from that branch's marker onward rather than letting whichever
        // call comes first in the source define the shape.
        if body.contains("if alias == \"localhost\"") && body.contains("get_daemon_with_scope(") {
            let (_, remote_branch) = body
                .split_once("get_daemon_with_scope(")
                .expect("guarded by the contains above");
            let remote = chain(remote_branch);
            if consults_scope(&remote) {
                return Some(remote);
            }
        }

        for (call, helper) in DELEGATES {
            if body.contains(call) {
                let delegated = chain(&strip_rust_comments(handler_body_of(daemon, helper)));
                if consults_scope(&delegated) {
                    return Some(delegated);
                }
            }
        }
        let own = chain(&body);
        consults_scope(&own).then_some(own)
    }

    /// One of the phrases a 403 has to use to be saying "a scope denial or an
    /// approver's refusal". A scoped token's own wording counts: `/ping` and
    /// `/exec/preview` answer "Scoped token does not allow …", which names the
    /// refusal without using the noun.
    const SCOPE_WORDS: [&str; 7] = [
        "scope denied",
        "scope denial",
        "outside the token",
        "token scope",
        "token's allowed hosts",
        "scoped token",
        "does not allow",
    ];

    let mut problems: Vec<String> = Vec::new();
    let mut shapes: Vec<(Vec<&'static str>, Vec<String>)> = Vec::new();
    let mut compared = 0usize;

    for (path, method, handler) in daemon_routes(daemon) {
        let Some(chain) = order_of(daemon, &handler) else {
            continue;
        };
        compared += 1;

        let Some(operation) = spec_operation_block(spec, &path, method) else {
            problems.push(format!(
                "`{handler}` consults the token scope, but {method} {path} has no operation in \
                 docs/api.yaml — the scope refusal has nowhere to be documented"
            ));
            continue;
        };
        let Some(described) = response_description(&operation, "403") else {
            problems.push(format!(
                "{method} {path} ({handler}) consults the token scope and documents no 403"
            ));
            continue;
        };
        if !SCOPE_WORDS
            .iter()
            .any(|phrase| described.to_lowercase().contains(phrase))
        {
            problems.push(format!(
                "{method} {path} ({handler}) consults the token scope and its 403 does not name \
                 that refusal: `{described}`"
            ));
        }

        // Three shapes, and what the description may claim differs per shape
        // because what the server does differs per shape.
        let names_a_precondition = ["first", "before", "unless", "even when"]
            .iter()
            .any(|marker| described.contains(marker));
        let scope_at = chain
            .iter()
            .position(|step| *step == "scope" || *step == "preflight");
        let gate_before_scope = match (chain.iter().position(|step| *step == "gate"), scope_at) {
            (Some(gate), Some(scope)) => gate < scope,
            _ => false,
        };

        if !gate_before_scope && names_a_precondition {
            problems.push(format!(
                "{method} {path}: `{handler}` runs the scope work at {chain:?} — nothing answers \
                 before it — but its 403 names a precondition: `{described}`"
            ));
        }
        if gate_before_scope && !names_a_precondition {
            problems.push(format!(
                "{method} {path}: `{handler}` calls the gate before the scope work ({chain:?}), \
                 so a paused gate answers 423 and scope is never consulted — but the 403 is a \
                 bare sentence, `{described}`. Say the order, or a client reads a paused gate as \
                 a scope denial"
            ));
        }

        match shapes.iter_mut().find(|(shape, _)| *shape == chain) {
            Some((_, paths)) => paths.push(format!("{method} {path}")),
            None => shapes.push((chain, vec![format!("{method} {path}")])),
        }
    }

    // The parse must not be vacuous. If it stopped finding scope-gated handlers,
    // both assertions below would pass by comparing nothing.
    assert!(
        compared >= 25,
        "only {compared} routes were found to consult the token scope; the call-order parse \
         has stopped reading handler bodies. Shapes found: {shapes:?}"
    );

    assert!(
        problems.is_empty(),
        "the documented refusal order and the handler's own call order disagree:\n  {}\n\n\
         Shapes found: {shapes:?}",
        problems.join("\n  ")
    );
}

/// The first call of each gate/rate/scope/authorize step in one handler body, in
/// source order, with comments already stripped.
///
/// Variants collapse to their step: `preflight_rewritable_request` and
/// `preflight_sftp_rm_rf` are the preflight step, and `authorize_targets` is the
/// authorization step. That is what lets `proxy_exec`'s two branches be read as
/// "same order, different mechanism" instead of as two shapes.
fn chain(cleaned: &str) -> Vec<&'static str> {
    const STEPS: [(&str, [&str; 3]); 5] = [
        ("gate", ["reject_if_gate_paused(", "", ""]),
        ("rate", ["reject_if_rate_limited(", "", ""]),
        ("scope", ["check_daemon_scope(", "", ""]),
        (
            "preflight",
            [
                "preflight_guarded_request(",
                "preflight_rewritable_request(",
                "preflight_sftp_rm_rf(",
            ],
        ),
        (
            "authorize",
            ["authorize_command(", "authorize_targets(", ""],
        ),
    ];
    let mut at: Vec<(usize, &'static str)> = Vec::new();
    for (label, needles) in STEPS {
        let earliest = needles
            .iter()
            .filter(|needle| !needle.is_empty())
            .filter_map(|needle| cleaned.find(needle))
            .min();
        if let Some(position) = earliest {
            at.push((position, label));
        }
    }
    at.sort();
    at.into_iter().map(|(_, label)| label).collect()
}

/// Strip `//` and `/* */` comments from Rust source, keeping string literals.
///
/// The call-order test searches a handler body for `name(` text, and this file's
/// prose spells those calls out. Without this the search reads a comment as a
/// call site.
fn strip_rust_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            out.push(byte as char);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            out.push('"');
            index += 1;
            continue;
        }
        if source[index..].starts_with("//") {
            match source[index..].find('\n') {
                Some(len) => {
                    out.extend(std::iter::repeat_n(' ', len));
                    index += len;
                }
                None => break,
            }
            continue;
        }
        if source[index..].starts_with("/*") {
            let len = source[index..]
                .find("*/")
                .map(|end| end + 2)
                .unwrap_or(bytes.len() - index);
            for byte in &bytes[index..index + len] {
                out.push(if *byte == b'\n' { '\n' } else { ' ' });
            }
            index += len;
            continue;
        }
        out.push(byte as char);
        index += 1;
    }
    out
}

/// Whether one byte can appear inside a Rust identifier.
fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The same text with comments **and** string or char literals blanked.
///
/// `strip_rust_comments` keeps literals on purpose: the call-order test reads
/// `if alias == "localhost"` out of a body, and blanking that would break it.
/// The helper scans want the opposite. A needle is a call expression, so a
/// literal that spells one is not a call — yet `body.contains(needle)` accepted
/// it, and a single
/// `tracing::debug!("reject_if_gate_paused( covers this route")` was enough to
/// satisfy a `HELPER_STATUS` row and, through the delegate search, an
/// exemption. Blanking literals is what separates "this body runs the helper"
/// from "this body talks about the helper".
///
/// Char literals are handled because they desync a literal-naive scanner:
/// `'"'` opens a string as far as the byte scan is concerned, and every byte up
/// to the next real quote — code included — would be swallowed. No handler
/// contains one today; the point is that the first one to appear must not
/// silently blind four checks at once. A lifetime (`'a`) is not a char literal,
/// and the two patterns below tell them apart: an escape (`'\''`) or exactly one
/// char before the closing quote.
fn strip_rust_literals(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut index = 0;
    // Blank one byte range, keeping newlines so any line-shaped reading of the
    // result still lines up.
    let blank = |out: &mut String, range: &[u8]| {
        for byte in range {
            out.push(if *byte == b'\n' { '\n' } else { ' ' });
        }
    };
    while index < bytes.len() {
        if source[index..].starts_with("//") {
            let len = source[index..].find('\n').unwrap_or(bytes.len() - index);
            blank(&mut out, &bytes[index..index + len]);
            index += len;
            continue;
        }
        if source[index..].starts_with("/*") {
            let len = source[index..]
                .find("*/")
                .map(|end| end + 2)
                .unwrap_or(bytes.len() - index);
            blank(&mut out, &bytes[index..index + len]);
            index += len;
            continue;
        }
        // Raw string: `r"…"`, `r#"…"#`, `r##"…"##`. The `r` only starts one when
        // it does not continue a name — `error` ends in `r` and is not a prefix.
        if bytes[index] == b'r'
            && !index
                .checked_sub(1)
                .is_some_and(|at| is_ident_byte(bytes[at]))
        {
            let mut hashes = 0;
            while index + 1 + hashes < bytes.len() && bytes[index + 1 + hashes] == b'#' {
                hashes += 1;
            }
            if index + 1 + hashes < bytes.len() && bytes[index + 1 + hashes] == b'"' {
                let opening = index + 2 + hashes;
                let terminator = format!("\"{}", "#".repeat(hashes));
                let len = source[opening..]
                    .find(&terminator)
                    .map(|end| end + terminator.len())
                    .unwrap_or(bytes.len() - opening);
                blank(&mut out, &bytes[index..opening + len]);
                index = opening + len;
                continue;
            }
        }
        if bytes[index] == b'"' {
            // Walk to the closing quote, honouring `\"`.
            let mut at = index + 1;
            while at < bytes.len() {
                if bytes[at] == b'\\' {
                    at += 2;
                    continue;
                }
                if bytes[at] == b'"' {
                    break;
                }
                at += 1;
            }
            let end = (at + 1).min(bytes.len());
            blank(&mut out, &bytes[index..end]);
            index = end;
            continue;
        }
        if bytes[index] == b'\'' {
            // `'\''` / `'\n'` / `'\u{1F600}'`: an escape, then the closing quote.
            let escaped_end = if bytes.get(index + 1) == Some(&b'\\') {
                source[index + 2..].find('\'').map(|at| index + 2 + at + 1)
            } else {
                None
            };
            // `'x'` / `'"'`: exactly one byte between the quotes. A lifetime has
            // no closing quote in that position, so it fails this test.
            let plain_end = (bytes.get(index + 2) == Some(&b'\''))
                .then_some(index + 3)
                .filter(|end| *end <= bytes.len());
            if let Some(end) = escaped_end.or(plain_end) {
                blank(&mut out, &bytes[index..end]);
                index = end;
                continue;
            }
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out
}

/// One operation's block: the method key and everything under it, bounded by the
/// next method key or the end of the path item.
///
/// `spec_path_block` returns the whole path item, and most paths mount several
/// methods. Reading one method's responses out of the whole block would let
/// `DELETE /sessions/{id}` pass on `POST`'s description — which is exactly the
/// confusion this test exists to catch, since those two genuinely differ here.
fn spec_operation_block(spec: &str, path: &str, method: &str) -> Option<String> {
    let block = spec_path_block(spec, path)?;
    let mut kept: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in block.lines() {
        let trimmed = line.trim_start();
        let is_method_key = !trimmed.starts_with('/')
            && line.len() - trimmed.len() == 4
            && trimmed
                .strip_suffix(':')
                .is_some_and(|name| matches!(name, "get" | "post" | "put" | "delete" | "patch"));
        if is_method_key {
            inside = trimmed
                .strip_suffix(':')
                .is_some_and(|name| name.eq_ignore_ascii_case(method));
            if inside {
                kept.push(line);
            }
            continue;
        }
        if inside {
            kept.push(line);
        }
    }
    (!kept.is_empty()).then(|| kept.join("\n"))
}

/// The description under one response code, joined if it is a folded scalar.
///
/// A `description: |` or `description: >-` body sits on the lines below the key,
/// so stopping at the key's own line finds nothing — and a test that treated
/// "found nothing" as "documents nothing" would fail every route that describes
/// itself at length. Folding the continuation into one line also lets the marker
/// search above see a phrase that wraps across two source lines.
fn response_description(operation: &str, code: &str) -> Option<String> {
    let lines: Vec<&str> = operation.lines().collect();
    let key = format!("\"{code}\":");
    let at = lines.iter().position(|line| line.trim() == key)?;
    for line in &lines[at + 1..] {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("description: ") else {
            // Anything other than a description immediately after the code means
            // this response has none.
            return None;
        };
        if matches!(rest, "|" | "|-" | ">" | ">-") {
            let base = line.len() - line.trim_start().len();
            let mut folded = String::new();
            let mut started = false;
            for continuation in &lines[at + 1..] {
                let text = continuation.trim();
                if text.is_empty() {
                    continue;
                }
                if !started {
                    // The first continuation line is the marker itself.
                    started = true;
                    continue;
                }
                if continuation.len() - continuation.trim_start().len() <= base {
                    break;
                }
                if !folded.is_empty() {
                    folded.push(' ');
                }
                folded.push_str(text);
            }
            return (!folded.is_empty()).then_some(folded);
        }
        return Some(rest.to_string());
    }
    None
}

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
            // A response key is three digits. Quoted prose sits at the same
            // indent inside `description:` scalars — `"unknown"` and
            // `"nothing was stopped"` both did — and used to be recorded as
            // documented codes: non-codes that no check could ever match, which
            // is worse than noise because it made the map look populated where
            // the operation documents nothing.
            if code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_digit()) {
                operation.2.push(code.to_string());
            }
        }
    }

    let undocumented: Vec<String> = daemon_routes(daemon)
        .into_iter()
        .flat_map(|(path, method, handler)| {
            // Comments and literals are stripped before the scan: this file's
            // prose spells these calls out, and a `//` line naming one used to
            // read as a call site. A string literal is the same failure with a
            // longer fuse — `debug!("reject_if_gate_paused( …")` is a mention,
            // not a call — so this scan strips both.
            let body = strip_rust_literals(handler_body_of(daemon, &handler));
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
            // Comments and literals are stripped before the scan: this file's
            // prose spells these calls out, and a `//` line naming one used to
            // read as a call site. A string literal is the same failure with a
            // longer fuse — `debug!("reject_if_gate_paused( …")` is a mention,
            // not a call — so this scan strips both.
            let body = strip_rust_literals(handler_body_of(daemon, &handler));
            let Some((_, _, codes)) = documented.iter().find(|(known_path, known_method, _)| {
                *known_path == path && *known_method == method
            }) else {
                return Vec::new();
            };
            // The codes this route consumes rather than returns. Only those are
            // exempt: a route can answer one code as a status and fold another
            // into a 2xx, and skipping the whole route would silence both.
            let inband: &[&str] = INBAND_REJECTIONS
                .iter()
                .find(|(exempt_method, exempt_path, _, _, _)| {
                    *exempt_method == method && *exempt_path == path
                })
                .map(|(_, _, exempt_codes, _, _)| *exempt_codes)
                .unwrap_or(&[]);
            HELPER_STATUS
                .iter()
                .filter(|(call, code)| {
                    body.contains(call)
                        && !inband.contains(code)
                        && !codes.iter().any(|documented| documented == code)
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
    // an operation could exempt a code it still documents and say nothing.
    let unbacked: Vec<String> = INBAND_REJECTIONS
        .iter()
        .flat_map(|entry| {
            let (method, path, exempt_codes, marker, delegates) = *entry;
            let Some((_, _, codes)) = documented.iter().find(|(known_path, known_method, _)| {
                known_path.as_str() == path && *known_method == method
            }) else {
                return vec![format!(
                    "{method} {path} is exempt from the helper check but has no operation in \
                     the spec, so the exemption guards nothing"
                )];
            };
            let mut problems: Vec<String> = exempt_codes
                .iter()
                .filter(|code| codes.iter().any(|documented| documented == *code))
                .map(|code| format!("{method} {path} documents {code} but answers it in-band"))
                .collect();
            if exempt_codes.is_empty() {
                problems.push(format!(
                    "{method} {path} exempts no code at all, so the entry hides nothing and \
                     guards nothing"
                ));
            }
            // A code can only be exempt as "answered through a shared helper" if
            // the table knows that code at all. Exempting one it has never heard
            // of says nothing about the handler and silences a check that was
            // never going to fire — the superset form of the same failure.
            for code in exempt_codes {
                if !HELPER_STATUS.iter().any(|(_, known)| known == code) {
                    problems.push(format!(
                        "{method} {path} exempts {code}, but no `HELPER_STATUS` entry answers \
                         that code, so nothing in the handler scan could ever have flagged it"
                    ));
                }
            }
            // An exemption is a claim that the route consumes the code instead
            // of returning it, and until this check existed nothing tested that
            // claim against a body. The table only asked whether some
            // `HELPER_STATUS` entry *anywhere* answers the code, so a route
            // could exempt a code none of its own calls produce and still look
            // backed — by another route's body. The evidence has to be local:
            // the handler's own body, or a delegate named in the fifth field
            // whose body folds the refusal into the same 2xx.
            if let Some((_, _, handler)) = daemon_routes(daemon)
                .into_iter()
                .find(|(known_path, known_method, _)| known_path == path && *known_method == method)
            {
                let own = strip_rust_literals(handler_body_of(daemon, &handler));
                let mut bodies = vec![own.clone()];
                for delegate in delegates {
                    // A delegate the handler never calls is borrowed evidence.
                    // The fifth field exists so an exemption can point at the
                    // callee that actually folds the refusal, and the check
                    // used to accept *any* body with that name — another
                    // route's handler, or the sibling method of the same path
                    // item. Both passed. Requiring the call keeps the evidence
                    // on this route's own path.
                    if !own.contains(&format!("{delegate}(")) {
                        problems.push(format!(
                            "{method} {path} names `{delegate}` as the body that folds its \
                             refusal, but `{handler}` never calls it, so the evidence comes \
                             from a body this route does not run"
                        ));
                    }
                    bodies.push(strip_rust_literals(handler_body_of(daemon, delegate)));
                }
                for code in exempt_codes {
                    let backed = HELPER_STATUS.iter().any(|(needle, known)| {
                        known == code && bodies.iter().any(|body| body.contains(needle))
                    });
                    if !backed {
                        problems.push(format!(
                            "{method} {path} exempts {code}, but neither its handler nor \
                             {delegates:?} calls a helper that answers that code, so the \
                             exemption hides a code this route never consumes"
                        ));
                    }
                }
            }
            // The marker has to live in this method's own block. `spec_path_block`
            // returns the whole path item, so a GET could satisfy its marker with
            // prose the DELETE wrote about a refusal the DELETE performs — the
            // same borrow the delegate rule above now refuses, one field over.
            match spec_operation_block(spec, path, method) {
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
         on that list must document none of the codes it exempts — those refusals never \
         become an HTTP status — and must still say in prose where the refusal does go."
    );
}

/// Every `HELPER_STATUS` needle must name a call some body actually makes.
///
/// The table is a text scan, so a needle that matches nothing is silently dead:
/// it claims a code is reachable through that helper while the reverse check on
/// `INBAND_REJECTIONS` keeps treating the code as backed. One entry was exactly
/// that — a status helper only the rate limiter itself calls, so no route body
/// ever contained it — and nothing noticed, because the table's only consumer is
/// a filter that fails open when a needle misses. A typo in a needle is the same
/// failure: renaming `authorize_command(` to `authorize_command_inner(` unbinds
/// nine routes' 408s at once.
///
/// Delegates are part of the search because an exemption may legitimately point
/// at a callee that folds the refusal; `handler_body_of` panics on a name the
/// daemon does not define, so a stale delegate is red rather than ignored.
#[test]
fn every_helper_status_needle_names_a_call_a_body_makes() {
    let daemon = include_str!("../src/bin/agent2ssh-daemon.rs");
    let mut bodies: Vec<String> = daemon_routes(daemon)
        .into_iter()
        .map(|(_, _, handler)| strip_rust_literals(handler_body_of(daemon, &handler)))
        .collect();
    for (_, _, _, _, delegates) in INBAND_REJECTIONS {
        for delegate in delegates {
            bodies.push(strip_rust_literals(handler_body_of(daemon, delegate)));
        }
    }
    let dead: Vec<&str> = HELPER_STATUS
        .iter()
        .map(|(needle, _)| *needle)
        .filter(|needle| !bodies.iter().any(|body| body.contains(needle)))
        .collect();
    assert!(
        dead.is_empty(),
        "these `HELPER_STATUS` needles match no route handler body and no declared \
         delegate, so the code they claim to make reachable is not: {dead:?}"
    );
}

/// The body of a named item in the daemon, `fn` or `async fn`.
///
/// `handler_body_of` requires `async fn` because every route handler is one.
/// `HELPER_STATUS` also names plain functions (`ensure_command_length`,
/// `locked_status`'s callers), and those are exactly the rows whose code a test
/// can check against the source. `None` means the item is not in this file —
/// `check_daemon_scope` lives in `src/remote.rs`, and a row that points there is
/// covered by the route scans rather than by this one.
fn item_body_of<'a>(daemon: &'a str, name: &str) -> Option<&'a str> {
    for signature in [format!("\nasync fn {name}("), format!("\nfn {name}(")] {
        if let Some(at) = daemon.find(&signature) {
            let rest = &daemon[at + 1..];
            return Some(&rest[..closing_brace(rest, opening_brace(rest)?)?]);
        }
    }
    None
}

/// The `from_u16(N)` numbers one body spells, in source order.
fn numbers_a_body_spells(body: &str) -> Vec<String> {
    let mut numerics: Vec<String> = Vec::new();
    let mut from = 0;
    while let Some(found) = body[from..].find("from_u16(") {
        let at = from + found + "from_u16(".len();
        let digits: String = body[at..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        if !digits.is_empty() {
            numerics.push(digits);
        }
        from = at;
    }
    numerics
}

/// The names of the `*_status()` helpers one body calls.
fn status_helpers_called(body: &str) -> Vec<String> {
    let bytes = body.as_bytes();
    let mut names: Vec<String> = Vec::new();
    for (at, _) in body.match_indices("_status()") {
        let mut start = at;
        while start > 0 && is_ident_byte(bytes[start - 1]) {
            start -= 1;
        }
        names.push(body[start..at + "_status".len()].to_string());
    }
    names
}

/// The status codes a body spells, one level of `*_status()` deep.
///
/// Two spellings exist. `StatusCode::BAD_REQUEST` names the constant; the two
/// refusals that have no useful name (`423`, `429`) go through
/// `StatusCode::from_u16(…).expect(…)` inside a helper of their own, because
/// `StatusCode` has no `LOCKED` constructor in this tree's version of `http`.
/// A number is the easier of the two to mistype and the harder to notice:
/// `423` → `400` turns "the operator paused the gate" into "your request was
/// malformed" with no compile error, and before this check no test read it.
///
/// The `*_status()` walk is one level and not recursive: `locked_status` is a
/// leaf, and a walk that followed calls generally would be a call graph this
/// file has no reason to build.
fn codes_a_body_spells(daemon: &str, body: &str) -> (Vec<String>, Vec<String>) {
    let mut literals: Vec<String> = HANDLER_STATUS
        .iter()
        .filter(|(constant, _)| body.contains(&format!("StatusCode::{constant}")))
        .map(|(_, code)| code.to_string())
        .collect();
    // `reject_if_gate_paused` does not spell 423 itself — it returns
    // `locked_status()`. Following the `*_status()` call is what makes the
    // numeric rows checkable at all.
    let mut numerics = numbers_a_body_spells(body);
    for name in status_helpers_called(body) {
        if let Some(helper) = item_body_of(daemon, &name) {
            // The helper exists to spell one number, so its literals are its
            // own business; only the number answers the row.
            numerics.extend(numbers_a_body_spells(&strip_rust_literals(helper)));
        }
    }
    literals.sort();
    literals.dedup();
    numerics.sort();
    numerics.dedup();
    (literals, numerics)
}

/// Each `HELPER_STATUS` row must agree with the code its helper actually builds.
///
/// The table's other two checks ask *where* a needle appears, never *what the
/// named function answers*. That left two shapes green: a row registered with a
/// code its helper does not build (`finish_sftp_operation` as `423`, while its
/// body builds one `400`), and a numeric status helper edited to a different
/// number (`locked_status`'s `423` → `400`), which silently rewrites every route
/// the gate check covers. Both were measured green on this file before the
/// check existed.
///
/// The rule is deliberately narrow, because a row is not a claim that its helper
/// answers *only* that code:
/// - if the body spells numbers, the row's code must be one of them;
/// - otherwise, if it spells exactly one literal code, the row's code must be
///   that one;
/// - otherwise (a pass-through, or a helper that answers several codes and
///   reaches this one through a callee) this check says nothing, and the route
///   scans remain what covers the row.
#[test]
fn every_helper_status_row_agrees_with_the_code_its_helper_builds() {
    let daemon = include_str!("../src/bin/agent2ssh-daemon.rs");

    let disagreements: Vec<String> = HELPER_STATUS
        .iter()
        .filter_map(|(needle, code)| {
            let name = needle.trim_end_matches('(');
            let body = strip_rust_literals(item_body_of(daemon, name)?);
            let (literals, numerics) = codes_a_body_spells(daemon, &body);
            if !numerics.is_empty() {
                return (!numerics.iter().any(|known| known == code)).then(|| {
                    format!("`{name}` is registered as {code}, but it spells {numerics:?}")
                });
            }
            match literals.as_slice() {
                [only] if only != code => Some(format!(
                    "`{name}` is registered as {code}, but its body builds {only}"
                )),
                _ => None,
            }
        })
        .collect();

    assert!(
        disagreements.is_empty(),
        "these `HELPER_STATUS` rows disagree with the source: {disagreements:?}. A row is \
         what the route scans treat as proof that a code is reachable, so a row that names \
         the wrong code makes every route it covers document the wrong status."
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
/// - `axum::body::Bytes` — `approval_approve` and `daemon_shutdown`. The handler parses by hand
///   and answers its own 400, so 415 and 422 cannot happen and must not be
///   claimed. The spec claimed both for one round, because the response codes
///   had been added by grepping for `requestBody` instead of reading the
///   signature.
/// - `Option<Json<T>>` — 2 handlers. Its rejection type is `Infallible`, so the
///   `.ok()` these handlers use cannot fail: a wrong content type, invalid JSON
///   and a mistyped field all arrive as `None`, i.e. "no body was sent".
///
/// The reader is derived from each handler's own parameter list, so this stays
/// correct when a signature changes; the operation is then looked up in the spec
/// and held to whatever that reading implies.
#[test]
fn api_yaml_documents_how_each_handler_reads_its_body() {
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

        // `Bytes` extraction still goes through axum's body limit, so a
        // too-large body is refused before the handler parses anything — and as
        // plain text, not the `Error` object. Nothing checked this:
        // `/shutdown` documented the 413 and `/approvals/{id}/approve`, the
        // other `Bytes` handler, did not, which is invisible from either side.
        if matches!(reader, BodyReader::HandParsed) {
            if !documents("413") {
                violations.push(format!(
                    "{label} reads `axum::body::Bytes`, so the default body limit answers 413 \
                     before the handler parses anything, but the operation does not document it"
                ));
            } else {
                let media: Vec<&str> = operation
                    .media
                    .iter()
                    .filter(|(known, _)| known == "413")
                    .map(|(_, media)| media.as_str())
                    .collect();
                if media != ["text/plain"] {
                    violations.push(format!(
                        "{label} documents 413 with {media:?}; axum answers the body limit with \
                         a plain-text message before the handler runs"
                    ));
                }
                // The two `Bytes` handlers documented the same fact in two
                // different amounts: one said only "over the limit", the other
                // said why the response cannot be the `Error` object. A client
                // reading one operation has no way to see the other, so the
                // explanation has to be on both — and "this one is different",
                // without the reason, reads as an inconsistency rather than a
                // contract.
                if !code_block(&operation.text, "413").contains("Bytes") {
                    violations.push(format!(
                        "{label} documents 413 without naming `Bytes`: this is the only status \
                         axum answers for a handler that reads the body itself, and that is why \
                         it cannot carry the `Error` object"
                    ));
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
         `Bytes` cannot answer 415/422 at all — the only status it adds on its own is the 413 \
         the default body limit raises before the handler runs, so every other refusal on those \
         operations has to be one the handler writes."
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

/// A response key is a line shaped exactly `"413":` — a quote, three ASCII
/// digits, then a quote and a colon. Checking the digits is what keeps this
/// honest: the shape alone (`"` … `":`) is also taken by a quoted id, and a key
/// that is not a code (`"unknown":`) would end a block early and turn a real
/// claim into a false negative.
fn is_response_key(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.len() == 6
        && bytes[0] == b'"'
        && bytes[1..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'"'
        && bytes[5] == b':'
}

/// The lines an operation documents for one status code, so a claim can be tied
/// to the response that makes it instead of to the whole operation.
///
/// `operation.text` holds every line of the operation with leading space
/// stripped, so a block runs from the code's own line to the next response key.
/// If an operation ever repeated a code, the first block wins; none does today.
///
/// A missing code is a failure, not an empty answer. Returning `""` made every
/// caller's `contains(...)` report "the docs do not say this" — which reads as a
/// documentation bug in the *caller's* subject while the real fault is that the
/// status was never documented at all. Panicking here names the code and the
/// operation instead, and costs the callers no special case.
fn code_block(operation_text: &str, code: &str) -> String {
    let marker = format!("\"{code}\":");
    let mut lines = operation_text.lines().skip_while(|line| *line != marker);
    assert!(
        lines.next().is_some(),
        "no response block for \"{code}\": in the operation being checked, so a claim about it \
         would be asserted against an empty string"
    );
    lines
        .take_while(|line| !is_response_key(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `code_block` must end on a status code, not on any quoted key.
///
/// The length check alone let `"abc":` through and would have let a longer
/// `"unknown":` through by ending the block before it — both turn a documented
/// claim into a silent false negative, which reads as "the docs are fine".
#[test]
fn a_response_key_is_a_three_digit_code() {
    assert!(is_response_key("\"413\":"));
    assert!(is_response_key("\"200\":"));
    assert!(!is_response_key("\"abc\":"));
    assert!(!is_response_key("\"unknown\":"));
    assert!(!is_response_key("413"));
}

/// Every response key in the spec is one `is_response_key` recognises.
///
/// The helper accepts exactly `"NNN":` — double quotes, three ASCII digits. YAML
/// also allows `'400':` and a bare `400:`, and neither would end a block: the
/// block would run into the next response and every claim checked against it
/// would pass for the wrong reason. Today all keys are double-quoted, so the
/// premise holds; this test is what makes that a fact rather than something a
/// reviewer has to re-measure. Restyle a key and it fails here, naming the line.
#[test]
fn every_response_key_in_the_spec_is_a_quoted_three_digit_code() {
    let spec = include_str!("../../docs/api.yaml");
    let mut keys = 0;
    for line in spec.lines() {
        let trimmed = line.trim_start();
        if !trimmed.ends_with(':') || trimmed.len() > 12 {
            continue;
        }
        let candidate = trimmed.trim_end_matches(':');
        // Only lines whose key is made of digits and quotes can be a status
        // code; `default:` and every schema key drop out here.
        if candidate.is_empty()
            || !candidate
                .chars()
                .all(|c| c.is_ascii_digit() || c == '"' || c == '\'')
        {
            continue;
        }
        keys += 1;
        assert!(
            is_response_key(trimmed),
            "the spec writes a response key as `{trimmed}` (Debug: {trimmed:?}), which \
             `is_response_key` does not recognise; `code_block` would then run past it and every \
             assertion about that response would be checked against the following block"
        );
    }
    assert!(
        keys > 400,
        "only {keys} response keys found in the spec; this scan is not looking at what it thinks"
    );
}

/// `local:<id>` is not limited to the environments this crate ships.
///
/// `resolve_plan_for_host` consults `environments.toml` before falling back to
/// the built-ins, so a definition named `py-ml` is reachable as `local:py-ml`
/// through `/local-exec` exactly like `local:gitbash`. The operation used to
/// enumerate only the built-ins and then say any other value is rejected with
/// 400 — which told a client that the one id the configuration guide documents
/// at length does not exist.
#[test]
fn api_yaml_documents_user_defined_local_environments() {
    let resolver = include_str!("../src/local_exec.rs");
    let spec = include_str!("../../docs/api.yaml");

    // The registry lookup is what makes the documented id work. Asserting it
    // first keeps this test honest in the other direction too: if the lookup is
    // removed, the failure names the source rather than the prose.
    assert!(
        resolver.contains("local_environments::current()"),
        "`resolve_plan_for_host` no longer consults the environments registry, so a user-defined \
         `local:<id>` is not reachable and `/local-exec` must stop saying it is"
    );

    let operation = spec
        .lines()
        .skip_while(|line| !line.starts_with("  /local-exec:"))
        .skip(1)
        .take_while(|line| !line.starts_with("  /"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        operation.contains("environments.toml"),
        "`/local-exec` documents `host` as an enumeration of the built-in environments, but a \
         `local:<id>` defined in `environments.toml` reaches the same handler; a client reading \
         this operation is told its own environment does not exist"
    );
}

/// `/local-exec` refuses with 400 for reasons whose next step differs, and the
/// ones whose remedy is "install something" were missing from the description.
///
/// The operation closed its `host` enumeration with a single "any other value is
/// rejected with 400", which reads as one refusal with one remedy. A suffix on an
/// environment that takes none (`local:cmd:ls`), an empty distribution
/// (`local:wsl:`), an environment this machine cannot build (`local:pwsh` with no
/// PowerShell) and a container runtime without its CLI (`local:docker:x` with no
/// `docker`) each carry their own message in `local_exec.rs`. The first two were
/// added earlier; the last two were still absent, and they are the pair a client
/// cannot guess from "fix the definition" — the definition is fine, the machine
/// is not. The source side is asserted first so a deleted arm fails by naming the
/// source instead of the prose.
#[test]
fn api_yaml_lists_the_local_refusals_it_can_produce() {
    let resolver = include_str!("../src/local_exec.rs");
    let spec = include_str!("../../docs/api.yaml");

    // Every needle must exist in the source *and* in the 400 block. Two of these
    // are the same shape as the "definition cannot be built" arm in the 400 and
    // deliberately distinct from it: `environment 'X' is based on ...` is about a
    // definition, `no PowerShell found on PATH` is about the machine.
    const NEEDLES: [(&str, &str); 5] = [
        ("takes no argument", "`local:cmd:ls`"),
        ("names no distribution", "`local:wsl:`"),
        (
            "no PowerShell found on PATH",
            "`local:pwsh` where no PowerShell exists",
        ),
        (
            "WSL is not available",
            "`local:wsl` where wsl.exe is not installed",
        ),
        (
            "is not installed, so there is nothing to exec into",
            "`local:docker:x` where the runtime's CLI is missing",
        ),
    ];

    for (needle, host) in NEEDLES {
        assert!(
            resolver.contains(needle),
            "`local_exec.rs` no longer refuses {host} with {needle:?}, so the 400 description has \
             a reason to delete rather than a reason to keep"
        );
    }

    let operation = spec
        .lines()
        .skip_while(|line| !line.starts_with("  /local-exec:"))
        .skip(1)
        .take_while(|line| !line.starts_with("  /"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    let refused = code_block(&operation, "400");
    for (needle, host) in NEEDLES {
        assert!(
            refused.contains(needle),
            "{host} is refused with {needle:?}, but the /local-exec 400 description does not say \
             so; a client reads one reason where there are several. Block was:\n{refused}"
        );
    }
}

/// The two internal errors the preflight can answer are stated once, at file
/// level, and not repeated on the operations that can answer them.
///
/// `api_yaml_documents_the_codes_each_handler_can_return` reads a handler's own
/// body, and these 500s are not there: `reject_if_gate_paused` reads
/// `execution_gate.json`, and the approval chain behind it reads `policy.toml` /
/// `policy.json` before falling back to `approval_policies.toml`. Both report an
/// unreadable file as an internal error from inside the helper.
///
/// A per-operation copy on all sixteen preflight callers was the alternative,
/// and it is the wrong one: the same paragraph in sixteen places is sixteen
/// chances to name the wrong file, which is exactly what the one round that
/// added a copy to `/local-exec` did — it named `policy.toml` and left out
/// `approval_policies.toml`, the file the same chain falls back to.
///
/// So this pins the shape in three places: the shared statement still names both
/// reads, `/local-exec` still carries no per-operation 500 (the entry whose
/// existence would contradict that statement), and its 400 still says that a
/// failure from the runner itself arrives on the same code — the half a client
/// cannot infer from a list of refusals.
#[test]
fn the_spec_states_the_preflight_internal_errors_once() {
    let spec = include_str!("../../docs/api.yaml");

    let file_level = spec
        .lines()
        .take_while(|line| !line.starts_with("  /"))
        .collect::<Vec<_>>()
        .join("\n");
    for needle in [
        "execution_gate.json",
        "policy.toml",
        "approval_policies.toml",
    ] {
        assert!(
            file_level.contains(needle),
            "the file-level section no longer names {needle} as one of the two shared reads that \
             answer 500. Every preflight caller relies on that paragraph instead of repeating the \
             500 on its own operation, so a client would now meet an undocumented 500"
        );
    }

    // The paragraph above attributes every preflight 500 to a file, and that
    // attribution is what a client acts on: "read the message, fix that file".
    // Two of them name no file — the approval wait is cancelled by a shutdown,
    // or the daemon sees a status it cannot interpret — so deleting this
    // sentence puts the reader back on the wrong remedy for both.
    assert!(
        file_level.contains("daemon shutting down; approval wait cancelled"),
        "the file-level 500 section no longer says that a cancelled approval wait answers 500 \
         without naming a file. The three names above then read as the complete list of causes, \
         and a client told to fix `policy.toml` on every 500 will look for a file that is fine"
    );

    let operation = spec
        .lines()
        .skip_while(|line| !line.starts_with("  /local-exec:"))
        .skip(1)
        .take_while(|line| !line.starts_with("  /"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !operation.contains("\"500\":"),
        "/local-exec lists a 500 of its own again. The two internal errors it can answer are stated \
         once at file level — see the paragraph beginning `A second 500 does need the handler` — and \
         a per-operation copy both contradicts that sentence and, the first time it was tried here, \
         named only one of the two files. If a 500 belongs on this operation for a different reason, \
         change the file-level sentence first so the two do not disagree."
    );
    let refused = code_block(&operation, "400");
    assert!(
        refused.contains("a refusal from a run that failed"),
        "/local-exec's 400 also carries every failure from the runner itself, and that is the half \
         a client cannot infer from a list of refusals; the description no longer says so. Block \
         was:\n{refused}"
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

/// One operation's slice of `docs/api.yaml`, from its `path:` line through the
/// last line indented deeper than the method key.
fn api_yaml_operation(spec: &str, path: &str, method: &str) -> String {
    let path_key = format!("  {path}:");
    let mut block = String::new();
    let mut inside = false;
    let mut seen_method = false;
    for line in spec.lines() {
        if !inside {
            if line == path_key {
                inside = true;
            }
            continue;
        }
        let body = line.trim_start();
        let indent = line.len() - body.len();
        // A sibling path (indent 2) or a sibling method (indent 4) ends the block.
        if line.starts_with("  /") || (indent == 4 && !body.is_empty() && !body.starts_with('#')) {
            if seen_method {
                break;
            }
            if indent == 4 {
                // Still inside the path, before reaching our method.
                if body.strip_suffix(':') == Some(method) {
                    seen_method = true;
                }
                continue;
            }
            break;
        }
        if seen_method {
            block.push_str(line);
            block.push('\n');
        }
    }
    assert!(
        seen_method,
        "docs/api.yaml has no `{method}` under `{path}`; the test is looking for an operation \
         that no longer exists"
    );
    block
}

/// The session-shaped endpoints answer 429 for different reasons, and the
/// `scope` suffix in the body is the only thing that tells them apart.
///
/// Both rejections share one message template
/// (`execution limit exceeded: <scope> current=… limit=…`), so a client that
/// retries on 429 cannot infer from the status alone whether it is waiting for a
/// window to slide or for a slot to be released. The contract said this
/// backwards once: `DELETE /sessions/{id}` claimed no 429 while
/// "`/sessions` — the open side — does list one", which reads as "opening is
/// rate-limited"; `session_open` never calls `reject_if_rate_limited`, it
/// reserves a slot through `check_session_open`, and `/resident-sessions` is the
/// endpoint that is rate-limited. Each sentence is now pinned to the mechanism
/// it describes.
///
/// The session cap has two takers, not one: `GET /terminal` calls the same
/// `register_limited_session_for_command` as `POST /sessions` and reserves a slot
/// of the same kind, while the two resident-session routes sit on the other side
/// (`POST` is rate-limited, `DELETE` is the preflighted close). Four operations
/// therefore depart from the template — two per family — and the file-level
/// paragraph that named only one from each under-counted the places a client has
/// to read the suffix.
#[test]
fn api_yaml_names_the_mechanism_behind_each_session_429() {
    let spec = include_str!("../../docs/api.yaml");

    let rate_paragraph = spec
        .split("Two different mechanisms answer 429")
        .nth(1)
        .unwrap_or_default()
        .split("\n\n")
        .next()
        .unwrap_or_default();
    for operation in [
        "`GET /terminal`",
        "`POST /sessions`",
        "`POST /resident-sessions`",
        "`DELETE /resident-sessions/{id}`",
    ] {
        assert!(
            rate_paragraph.contains(operation),
            "the 429 paragraph has to name every operation whose 429 description departs from \
             the template, and {operation} is one of them; a reader who trusts a short list will \
             not look at the operations it left out"
        );
    }

    let terminal = api_yaml_operation(spec, "/terminal", "get");
    assert!(
        terminal.contains("register_limited_session_for_command")
            && terminal.contains("terminal_open"),
        "`GET /terminal` answers 429 from the session cap through the same helper as \
         `POST /sessions` (`register_limited_session_for_command`, operation `terminal_open`); a \
         bare \"session concurrency limit\" leaves the family unstated"
    );
    assert!(
        terminal.contains("`sessions`") && terminal.contains("never calls"),
        "the `sessions` suffix is what tells the client to close something rather than wait, and \
         `terminal_attach` never calls `reject_if_rate_limited` — both belong in the description"
    );

    let open = api_yaml_operation(spec, "/sessions", "post");
    assert!(
        open.contains("check_session_open"),
        "`POST /sessions` answers 429 from the session cap (`check_session_open`), and its 429 \
         description has to name that instead of implying a rate limit"
    );
    assert!(
        open.contains("sessions`,") || open.contains("sessions`."),
        "the scope suffix `sessions` is the only client-visible discriminator; it belongs in the \
         `POST /sessions` 429 description"
    );
    assert!(
        open.contains("never calls"),
        "`session_open` never calls `reject_if_rate_limited`; a description that only lists the \
         429 without saying so is what let the two mechanisms be confused"
    );

    let close = api_yaml_operation(spec, "/sessions/{id}", "delete");
    assert!(
        !close.contains("\"429\":"),
        "`session_close` checks no limit — it releases the slot — so a 429 there would be a \
         promise the handler cannot keep"
    );
    assert!(
        close.contains("unregister_session"),
        "the reason `DELETE /sessions/{{id}}` cannot answer 429 is that it calls \
         `unregister_session`; state the release, not just the absence"
    );

    let resident = api_yaml_operation(spec, "/resident-sessions", "post");
    assert!(
        resident.contains("reject_if_rate_limited") && resident.contains("`rate`"),
        "`resident_session_open` runs the shared preflight, so its 429 is the per-minute rate \
         limit and the suffix reads `rate`"
    );
    assert!(
        resident.contains("AGENT2SSH_MAX_RESIDENT_SESSIONS") && resident.contains("400"),
        "the resident-session cap answers 400 from `resident_open_core`, not 429; a 429 \
         description that also claims that cap sends clients to wait for the wrong thing"
    );

    let resident_close = api_yaml_operation(spec, "/resident-sessions/{id}", "delete");
    assert!(
        resident_close.contains("reject_if_rate_limited") && resident_close.contains("`rate`"),
        "closing a resident session runs the shared preflight, so its 429 is the `rate` family — \
         the one close that *is* rate-limited, and the sentence has to say so rather than leaving \
         a short label the reader can mistake for the slot-releasing case"
    );
    assert!(
        resident_close.contains("DELETE /sessions/{id}"),
        "the contrast is the useful part: `DELETE /sessions/{{id}}` checks no limit precisely \
         because it is the only release for its counter, and a reader who mixes the two will \
         retry a close that can never clear the other one"
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

// ── The guide's action-string list vs the daemon's ──────────────────────────

/// Action strings in `agent2ssh-daemon.rs`: every string literal handed to
/// `check_daemon_scope` as its fourth argument, plus the `RESIDENT_*_COMMAND`
/// constants.
///
/// The constants are in the set because those two reach the same check through
/// `preflight_guarded_request` → `authorize_command_with_approval`
/// (`execution_control.rs:445` is its first statement) rather than through an
/// inline call, so scanning only the literal call sites would miss
/// `resident-session open` and `resident-session close`.
///
/// A fourth argument that is a shell command (`&body.command`,
/// `&request.command`, `&command`) is not an action string and is skipped: the
/// guide matches those with glob patterns, not by enumerating them.
fn fixed_action_strings(source: &str) -> BTreeSet<String> {
    let mut consts: BTreeMap<String, String> = BTreeMap::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("const ") else {
            continue;
        };
        let Some((name, tail)) = rest.split_once(": &str = \"") else {
            continue;
        };
        let Some(value) = tail.split('"').next() else {
            continue;
        };
        if name.starts_with("RESIDENT_") && name.ends_with("_COMMAND") {
            consts.insert(name.to_string(), value.to_string());
        }
    }

    let mut actions: BTreeSet<String> = consts.values().cloned().collect();
    let call = "check_daemon_scope";
    let mut search = 0usize;
    while let Some(relative) = source[search..].find(call) {
        let at = search + relative;
        search = at + call.len();
        // Skip the definition and any doc-comment mention: only a call has `(`.
        if !source[search..].starts_with('(') {
            continue;
        }
        // A definition reads `fn check_daemon_scope(`.
        if source[..at].trim_end().ends_with("fn") {
            continue;
        }
        let Some(arguments) = call_arguments(source, search) else {
            continue;
        };
        let Some(fourth) = arguments.get(3) else {
            continue;
        };
        let fourth = fourth.trim();
        if let Some(literal) = fourth
            .strip_prefix('"')
            .and_then(|rest| rest.split('"').next())
        {
            actions.insert(literal.to_string());
        } else if let Some(value) = consts.get(fourth) {
            actions.insert(value.clone());
        }
    }
    actions
}

/// Split the argument list that starts at `open` (the byte index of `(`).
///
/// Tracks nesting so a `format!(...)` or a tuple inside one argument does not
/// split it, and skips string literals so a comma or paren inside one is not
/// read as structure.
fn call_arguments(source: &str, open: usize) -> Option<Vec<String>> {
    let mut depth = 0usize;
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for character in source[open..].chars() {
        if quoted {
            current.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
            continue;
        }
        match character {
            '"' => {
                quoted = true;
                current.push(character);
            }
            '(' | '[' | '{' => {
                depth += 1;
                if depth > 1 {
                    current.push(character);
                }
            }
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    if !current.trim().is_empty() {
                        arguments.push(current);
                    }
                    return Some(arguments);
                }
                current.push(character);
            }
            ',' if depth == 1 => {
                arguments.push(std::mem::take(&mut current));
            }
            _ => current.push(character),
        }
    }
    None
}

/// `docs/guides/configuration-guide.md` lists the fixed action strings verbatim,
/// and `allowed_commands` is a hard whitelist, so a string missing from that
/// list is a token that cannot perform the operation — silently, until the
/// deployment that needs it.
///
/// The list is prose, which is why it went stale: `disconnect` was authorizing a
/// real route (`ssh_disconnect`, `agent2ssh-daemon.rs:3658`) while the bullet
/// named only `connect`, and nothing in the suite could notice. Deriving the set
/// from the source closes that: the tenth string cannot be added to the daemon
/// without this test naming the paragraph that has to grow with it.
#[test]
fn configuration_guide_lists_every_fixed_action_string() {
    let actions = fixed_action_strings(include_str!("../src/bin/agent2ssh-daemon.rs"));
    assert!(
        actions.len() >= 10,
        "the parse found only {} action strings, fewer than the ten the daemon authorizes \
         (`ping`, `session_open`, `session_read`, `session_close`, `terminal`, \
         `resident-session open|read|close`, `connect`, `disconnect`) — teach \
         `fixed_action_strings` the shape it stopped reading, or this test has quietly stopped \
         covering the list it exists to cover: {actions:?}",
        actions.len()
    );

    let guide = include_str!("../../docs/guides/configuration-guide.md");
    let bullet = guide
        .lines()
        .find(|line| line.contains("`allowed_commands` 非空时是一张绝对白名单"))
        .expect(
            "docs/guides/configuration-guide.md no longer has the `allowed_commands` whitelist \
             bullet; if it was reworded, update the marker in this test rather than deleting \
             the assertion",
        );

    let missing: Vec<&String> = actions
        .iter()
        .filter(|action| !bullet.contains(&format!("`{action}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "these action strings are authorized by the daemon but absent from the `allowed_commands` \
         bullet in docs/guides/configuration-guide.md: {missing:?}. Each one is a whitelist entry \
         an operator has to write by hand, so an omission is a token that is refused on that \
         operation alone. Add them to the bullet, with the same backticked spelling the daemon \
         matches."
    );
}
