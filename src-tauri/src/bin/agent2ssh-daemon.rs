use agent2ssh::approval::{approval_list, approval_request, approval_request_with_context, approval_respond, approval_wait, ApprovalStatus};
use agent2ssh::approval::ApprovalRequest as ApprovalRequestType;
use agent2ssh::approval::{
    check_approval_required, list_approval_policies, save_approval_policies, ApprovalPolicy,
    approval_action_url, build_approval_context,
};
use agent2ssh::connection::{connect_host, disconnect_host, list_active_connections};
use agent2ssh::core::*;
use agent2ssh::forward::*;
use agent2ssh::health::{collect_health_snapshot, load_health_snapshot};
use agent2ssh::notify::{fire_webhook, load_webhook_config, save_webhook_config, WebhookConfig, WebhookEvent, notify_approval_pending};
use agent2ssh::playbook::{
    dry_run_playbook, list_playbooks_core, run_playbook_core, Playbook, PlaybookDryRun,
    PlaybookRunResult,
};
use agent2ssh::remote::{get_daemon, list_daemons_core, diagnose_daemon, check_daemon_version, get_daemons_unified_view};
use agent2ssh::risk_config::classify_with_user_rules;
use agent2ssh::session::*;
use agent2ssh::store::*;
use agent2ssh::types::*;
use agent2ssh::events::subscribe_events;

use axum::{
    extract::{Path, Query, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, sse::{Sse, Event, KeepAlive}},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tracing;
use uuid::Uuid;

// ── Metrics counters ─────────────────────────────────────────────────────────

static REQUEST_COUNT: AtomicU64 = AtomicU64::new(0);
static EXEC_COUNT: AtomicU64 = AtomicU64::new(0);
static EXEC_BLOCKED_COUNT: AtomicU64 = AtomicU64::new(0);
static EXEC_TOTAL_DURATION_MS: AtomicU64 = AtomicU64::new(0);
static APPROVAL_COUNT: AtomicU64 = AtomicU64::new(0);

// ── Uptime tracking ──────────────────────────────────────────────────────────

static START_TIME: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn uptime_secs() -> u64 {
    START_TIME
        .get_or_init(Instant::now)
        .elapsed()
        .as_secs()
}

// ── State ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    token: String,
}

// ── Auth ─────────────────────────────────────────────────────────────────────

fn check_auth(state: &AppState, headers: &HeaderMap) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if token == state.token {
        Ok(())
    } else {
        tracing::warn!("failed authentication attempt");
        Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody { error: "unauthorized".into() }),
        ))
    }
}

// ── Error type ───────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ErrorBody { error: String }

fn err(status: StatusCode, msg: impl ToString) -> (StatusCode, Json<ErrorBody>) {
    (status, Json(ErrorBody { error: msg.to_string() }))
}

// ── Request/Response types ───────────────────────────────────────────────────

#[derive(Deserialize)] struct PingBody { hosts: Vec<String>, timeout_secs: Option<u64> }
#[derive(Deserialize)] struct ExecMultiBody { hosts: Vec<String>, command: String, #[serde(default)] force: bool, timeout_secs: Option<u64>, #[serde(default)] tags: Option<Vec<String>>, #[serde(default)] strategy: Option<BatchStrategy> }
#[derive(Deserialize)] struct ExecCompareBody { hosts: Vec<String>, command: String, #[serde(default)] force: bool, timeout_secs: Option<u64>, #[serde(default)] tags: Option<Vec<String>> }
#[derive(Deserialize)] struct SftpDirBody { host: String, path: String }
#[derive(Deserialize)] struct SessionOpenBody { host: String }
#[derive(Deserialize)] struct SessionWriteBody { input: String }
#[derive(Deserialize)] struct ReadQuery { timeout_ms: Option<u64> }
#[derive(Deserialize)] struct AuditQuery { host: Option<String>, risk_level: Option<RiskLevel>, exit_code: Option<i32>, since: Option<String>, until: Option<String>, limit: Option<usize>, search: Option<String>, command_pattern: Option<String>, host_env: Option<String>, host_role: Option<String>, host_owner: Option<String> }
#[derive(Deserialize)] struct AuditExportQuery { host: Option<String>, risk_level: Option<RiskLevel>, exit_code: Option<i32>, since: Option<String>, until: Option<String>, limit: Option<usize>, search: Option<String>, command_pattern: Option<String>, host_env: Option<String>, host_role: Option<String>, host_owner: Option<String>, format: Option<String> }
#[derive(Deserialize)] struct RiskCheckBody { command: String, #[allow(dead_code)] host: Option<String> }
#[derive(Deserialize)] struct PlaybookRunBody { playbook: String, host: String, #[serde(default)] force: bool, #[serde(default)] params: Option<HashMap<String, String>> }
#[derive(Deserialize)] struct PlaybookDryRunBody { playbook: String, #[serde(default)] params: Option<HashMap<String, String>> }
#[derive(Deserialize)] struct ExecPreviewBody { host: Option<String>, hosts: Option<Vec<String>>, command: String, timeout_secs: Option<u64>, #[serde(default)] tags: Option<Vec<String>> }
#[derive(Deserialize, Default)] struct HealthSnapshotBody { #[serde(default)] hosts: Option<Vec<String>>, timeout_secs: Option<u64> }
#[derive(Serialize)] struct RiskCheckResult { risk_level: RiskLevel, matched_rule: Option<String> }
#[derive(Serialize)] struct OkBody { ok: bool }
#[derive(Serialize)] struct IdBody { id: String }
#[derive(Serialize)] struct SessionListItem { id: String, host: String }
#[derive(Serialize)] struct OutputBody { output: String }

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn health() -> Json<serde_json::Value> {
    let config_ok = config_dir().map(|d| d.exists()).unwrap_or(false);
    let ssh_ok = which_binary("ssh").is_some();
    Json(serde_json::json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": uptime_secs(),
        "config_dir_available": config_ok,
        "ssh_available": ssh_ok,
        "pid": std::process::id(),
    }))
}

async fn metrics() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "requests_total": REQUEST_COUNT.load(Ordering::Relaxed),
        "exec_total": EXEC_COUNT.load(Ordering::Relaxed),
        "exec_blocked_total": EXEC_BLOCKED_COUNT.load(Ordering::Relaxed),
        "exec_total_duration_ms": EXEC_TOTAL_DURATION_MS.load(Ordering::Relaxed),
        "approvals_total": APPROVAL_COUNT.load(Ordering::Relaxed),
    }))
}

async fn list_hosts(
    State(s): State<AppState>, headers: HeaderMap,
) -> Result<Json<Vec<HostProfile>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    list_hosts_core().map(Json).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn add_host(
    State(s): State<AppState>, headers: HeaderMap, Json(host): Json<HostProfile>,
) -> Result<Json<HostProfile>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    add_host_core(host).map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

async fn remove_host(
    State(s): State<AppState>, headers: HeaderMap, Path(name): Path<String>,
) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    remove_host_core(&name).map(|_| Json(OkBody { ok: true })).map_err(|e| err(StatusCode::NOT_FOUND, e))
}

async fn import_config(
    State(s): State<AppState>, headers: HeaderMap,
) -> Result<Json<Vec<HostProfile>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    import_ssh_config_core(None).map(Json).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn ping(
    State(s): State<AppState>, headers: HeaderMap, Json(body): Json<PingBody>,
) -> Result<Json<Vec<PingResult>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    Ok(Json(ping_hosts_core(body.hosts, body.timeout_secs).await))
}

async fn exec(
    State(s): State<AppState>, headers: HeaderMap, Json(req): Json<ExecRequest>,
) -> Result<Json<ExecResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    tracing::info!(host = %req.host, command = %req.command, "exec handler invoked");

    let exec_start = Instant::now();

    // Check user-defined rules first
    if let Some(user_risk) = classify_with_user_rules(&req.command).await {
        if user_risk == RiskLevel::Blocked {
            EXEC_BLOCKED_COUNT.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(host = %req.host, command = %req.command, "command blocked by user rule");
            let evt = WebhookEvent {
                event: "exec_blocked".into(),
                host: req.host.clone(),
                command: req.command.clone(),
                approval_id: None,
                risk_level: Some("blocked".into()),
                exit_code: None,
            };
            tokio::spawn(async move {
                if let Err(e) = fire_webhook(evt).await {
                    tracing::error!(error = %e, "webhook fire error");
                }
            });
            return Err(err(StatusCode::BAD_REQUEST, format!("command blocked by user rule: '{}'", req.command)));
        }
    }
    // Determine effective risk level
    let base_risk = classify_risk(&req.command);
    let effective_risk = if let Some(user_risk) = classify_with_user_rules(&req.command).await {
        match (&user_risk, &base_risk) {
            (RiskLevel::Blocked, _) => RiskLevel::Blocked,
            (RiskLevel::High, RiskLevel::Blocked) => RiskLevel::Blocked,
            (ur, _) => *ur,
        }
    } else { base_risk };

    // If high risk and not forced, require approval
    if effective_risk == RiskLevel::High && !req.force {
        APPROVAL_COUNT.fetch_add(1, Ordering::Relaxed);

        // Build approval context with host details, history, and risk breakdown
        let approval_ctx = build_approval_context(&req.host, &req.command, "daemon")
            .ok()
            .map(|mut ctx| {
                ctx.reason = req.reason.clone();
                ctx.change_id = req.change_id.clone();
                ctx
            });

        let approval_id = if let Some(ctx) = approval_ctx {
            approval_request_with_context(&req.host, &req.command, effective_risk, 300, ctx).await
        } else {
            approval_request(&req.host, &req.command, effective_risk).await
        };

        let host_clone = req.host.clone();
        let cmd_clone = req.command.clone();
        let approval_id_str = approval_id.to_string();
        let risk_str = format!("{:?}", effective_risk).to_lowercase();

        // Build the approval action URL for notification
        let action_url = approval_action_url("http://127.0.0.1:7722", &approval_id_str);

        // Fire approval_required webhook with approval-specific notification
        let evt = WebhookEvent {
            event: "approval_required".into(),
            host: host_clone.clone(),
            command: cmd_clone.clone(),
            approval_id: Some(approval_id_str.clone()),
            risk_level: Some(risk_str.clone()),
            exit_code: None,
        };

        let action_url_clone = action_url.clone();
        let host_notify = host_clone.clone();
        let cmd_notify = cmd_clone.clone();
        let risk_notify = risk_str.clone();
        let approval_id_notify = approval_id_str.clone();
        tokio::spawn(async move {
            // Fire the new approval-specific notification
            if let Err(e) = notify_approval_pending(
                &approval_id_notify,
                &host_notify,
                &cmd_notify,
                &risk_notify,
                Some(&action_url_clone),
            ).await {
                tracing::error!(error = %e, "approval notification error");
            }
            // Also fire the legacy webhook event
            if let Err(e) = fire_webhook(evt).await {
                tracing::error!(error = %e, "webhook fire error");
            }
        });

        let status = approval_wait(approval_id).await;
        match status {
            ApprovalStatus::Approved => {
                // Execute with force
                let mut approved_req = req;
                approved_req.force = true;
                let result = exec_ssh_core(approved_req).await;
                EXEC_COUNT.fetch_add(1, Ordering::Relaxed);
                EXEC_TOTAL_DURATION_MS.fetch_add(exec_start.elapsed().as_millis() as u64, Ordering::Relaxed);
                let exit_code = result.as_ref().ok().and_then(|r| r.exit_code);
                let evt = WebhookEvent {
                    event: "exec_completed".into(),
                    host: host_clone,
                    command: cmd_clone,
                    approval_id: Some(approval_id_str),
                    risk_level: None,
                    exit_code,
                };
                tokio::spawn(async move {
                    if let Err(e) = fire_webhook(evt).await {
                        tracing::error!(error = %e, "webhook fire error");
                    }
                });
                result.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
            }
            ApprovalStatus::Rejected => {
                let evt = WebhookEvent {
                    event: "exec_completed".into(),
                    host: host_clone,
                    command: cmd_clone,
                    approval_id: Some(approval_id_str),
                    risk_level: None,
                    exit_code: None,
                };
                tokio::spawn(async move {
                    if let Err(e) = fire_webhook(evt).await {
                        tracing::error!(error = %e, "webhook fire error");
                    }
                });
                Err(err(StatusCode::FORBIDDEN, "command rejected by approver"))
            }
            ApprovalStatus::TimedOut => {
                let evt = WebhookEvent {
                    event: "exec_completed".into(),
                    host: host_clone,
                    command: cmd_clone,
                    approval_id: Some(approval_id_str),
                    risk_level: None,
                    exit_code: None,
                };
                tokio::spawn(async move {
                    if let Err(e) = fire_webhook(evt).await {
                        tracing::error!(error = %e, "webhook fire error");
                    }
                });
                Err(err(StatusCode::REQUEST_TIMEOUT, "approval request timed out"))
            }
            _ => {
                tracing::error!("unexpected approval status");
                Err(err(StatusCode::INTERNAL_SERVER_ERROR, "unexpected approval status"))
            }
        }
    } else {
        let host_clone = req.host.clone();
        let cmd_clone = req.command.clone();
        let result = exec_ssh_core(req).await;
        EXEC_COUNT.fetch_add(1, Ordering::Relaxed);
        EXEC_TOTAL_DURATION_MS.fetch_add(exec_start.elapsed().as_millis() as u64, Ordering::Relaxed);
        let exit_code = result.as_ref().ok().and_then(|r| r.exit_code);
        let evt = WebhookEvent {
            event: "exec_completed".into(),
            host: host_clone,
            command: cmd_clone,
            approval_id: None,
            risk_level: None,
            exit_code,
        };
        tokio::spawn(async move {
            if let Err(e) = fire_webhook(evt).await {
                tracing::error!(error = %e, "webhook fire error");
            }
        });
        result.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
    }
}

async fn exec_multi(
    State(s): State<AppState>, headers: HeaderMap, Json(body): Json<ExecMultiBody>,
) -> Result<Json<ExecMultiBatchResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    tracing::info!(hosts = ?body.hosts, command = %body.command, "exec-multi handler invoked");
    Ok(Json(exec_multi_with_strategy(body.hosts, body.command, body.force, body.timeout_secs, body.tags, body.strategy).await))
}

async fn exec_compare(
    State(s): State<AppState>, headers: HeaderMap, Json(body): Json<ExecCompareBody>,
) -> Result<Json<ExecComparison>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    tracing::info!(hosts = ?body.hosts, command = %body.command, "exec-compare handler invoked");
    let results = exec_multi_core(body.hosts, body.command, body.force, body.timeout_secs, body.tags).await;
    Ok(Json(compare_exec_results(&results)))
}

async fn audit(
    State(s): State<AppState>, headers: HeaderMap, Query(q): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEntry>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let filter = AuditFilter { host: q.host, risk_level: q.risk_level, exit_code: q.exit_code, since: q.since, until: q.until, limit: q.limit.unwrap_or(20), search: q.search, command_pattern: q.command_pattern, host_env: q.host_env, host_role: q.host_role, host_owner: q.host_owner };
    list_audit_core(filter).map(Json).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

// ── Audit Export (F6-2) ─────────────────────────────────────────────────────

async fn audit_export(
    State(s): State<AppState>, headers: HeaderMap, Query(q): Query<AuditExportQuery>,
) -> Result<axum::response::Response, (StatusCode, Json<ErrorBody>)> {
    use axum::response::IntoResponse;
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let filter = AuditFilter {
        host: q.host,
        risk_level: q.risk_level,
        exit_code: q.exit_code,
        since: q.since,
        until: q.until,
        limit: q.limit.unwrap_or(20),
        search: q.search,
        command_pattern: q.command_pattern,
        host_env: q.host_env,
        host_role: q.host_role,
        host_owner: q.host_owner,
    };
    let format = q.format.unwrap_or_else(|| "jsonl".to_string());
    match format.to_lowercase().as_str() {
        "jsonl" => {
            let data = export_audit_jsonl(&filter)
                .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
            let mut headers = axum::http::HeaderMap::new();
            headers.insert("content-type", "application/x-ndjson".parse().unwrap());
            Ok((StatusCode::OK, headers, data).into_response())
        }
        "csv" => {
            let data = export_audit_csv(&filter)
                .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
            let mut headers = axum::http::HeaderMap::new();
            headers.insert("content-type", "text/csv".parse().unwrap());
            Ok((StatusCode::OK, headers, data).into_response())
        }
        other => Err(err(
            StatusCode::BAD_REQUEST,
            format!("unsupported format '{}', expected 'jsonl' or 'csv'", other),
        )),
    }
}

// ── SFTP ─────────────────────────────────────────────────────────────────────

async fn sftp_upload(State(s): State<AppState>, headers: HeaderMap, Json(req): Json<SftpUploadRequest>) -> Result<Json<SftpResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?; sftp_upload_core(req).await.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn sftp_download(State(s): State<AppState>, headers: HeaderMap, Json(req): Json<SftpDownloadRequest>) -> Result<Json<SftpResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?; sftp_download_core(req).await.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn sftp_ls(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<SftpDirBody>) -> Result<Json<ExecResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?; sftp_ls_core(&body.host, &body.path, None).await.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn sftp_stat(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<SftpDirBody>) -> Result<Json<ExecResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?; sftp_stat_core(&body.host, &body.path, None).await.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn sftp_mkdir(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<SftpDirBody>) -> Result<Json<ExecResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?; sftp_mkdir_core(&body.host, &body.path, None).await.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

// ── Sessions ─────────────────────────────────────────────────────────────────

async fn session_open(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<SessionOpenBody>) -> Result<Json<IdBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    tracing::info!(host = %body.host, "session_open invoked");
    session_open_core(&body.host).await.map(|id| Json(IdBody { id: id.to_string() })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn session_write(State(s): State<AppState>, headers: HeaderMap, Path(id): Path<String>, Json(body): Json<SessionWriteBody>) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    session_write_core(uuid, &body.input).await.map(|_| Json(OkBody { ok: true })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn session_read(State(s): State<AppState>, headers: HeaderMap, Path(id): Path<String>, Query(q): Query<ReadQuery>) -> Result<Json<OutputBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    session_read_core(uuid, q.timeout_ms.unwrap_or(2000)).await.map(|output| Json(OutputBody { output })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn session_close(State(s): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    session_close_core(uuid).await.map(|_| Json(OkBody { ok: true })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn session_list(State(s): State<AppState>, headers: HeaderMap) -> Result<Json<Vec<SessionListItem>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    Ok(Json(session_list_core().await.into_iter().map(|(id, host)| SessionListItem { id: id.to_string(), host }).collect()))
}

// ── Forwards ─────────────────────────────────────────────────────────────────

async fn forward_add(State(s): State<AppState>, headers: HeaderMap, Json(req): Json<ForwardRule>) -> Result<Json<ForwardRule>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    forward_add_core(&req.host, req.direction, req.bind_port, &req.target_host, req.target_port).await.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn forward_list(State(s): State<AppState>, headers: HeaderMap) -> Result<Json<Vec<ForwardRule>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    Ok(Json(forward_list_core().await))
}
async fn forward_remove(State(s): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    forward_remove_core(uuid).await.map(|_| Json(OkBody { ok: true })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

// ── Approvals ────────────────────────────────────────────────────────────────

async fn approvals_list(State(s): State<AppState>, headers: HeaderMap) -> Result<Json<Vec<ApprovalRequestType>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    Ok(Json(approval_list().await))
}
async fn approval_approve(State(s): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    approval_respond(uuid, true).await.map(|_| Json(OkBody { ok: true })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn approval_reject(State(s): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    approval_respond(uuid, false).await.map(|_| Json(OkBody { ok: true })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

#[derive(Deserialize)] struct ApprovalRespondBody { approved: bool }

/// Generic approval respond endpoint used by notification action URLs.
/// Accepts a JSON body with `approved: bool`.
async fn approval_respond_generic(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ApprovalRespondBody>,
) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    approval_respond(uuid, body.approved).await.map(|_| Json(OkBody { ok: true })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

// ── Approval policies ────────────────────────────────────────────────────────

#[derive(Deserialize)] struct ApprovalCheckBody { host: String, command: String }
#[derive(Serialize)] struct ApprovalCheckResult { requires_approval: bool, matched_policy: Option<String>, risk_level: RiskLevel, ttl_secs: Option<u64> }

async fn list_policies(
    State(s): State<AppState>, headers: HeaderMap,
) -> Result<Json<Vec<ApprovalPolicy>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    list_approval_policies().map(Json).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn put_policies(
    State(s): State<AppState>, headers: HeaderMap, Json(policies): Json<Vec<ApprovalPolicy>>,
) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    save_approval_policies(&policies).map(|_| Json(OkBody { ok: true })).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn approval_check(
    State(s): State<AppState>, headers: HeaderMap, Json(body): Json<ApprovalCheckBody>,
) -> Result<Json<ApprovalCheckResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let host_tags: Vec<String> = load_config()
        .ok()
        .and_then(|c| c.hosts.into_iter().find(|h| h.name == body.host))
        .map(|h| h.tags)
        .unwrap_or_default();
    let risk = classify_risk(&body.command);
    let result = check_approval_required(&body.host, &host_tags, &body.command, risk)
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

// ── Risk check ───────────────────────────────────────────────────────────────

async fn risk_check(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<RiskCheckBody>) -> Result<Json<RiskCheckResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let base = classify_risk(&body.command);
    if let Some(user_risk) = classify_with_user_rules(&body.command).await {
        let final_risk = match (&user_risk, &base) {
            (RiskLevel::Blocked, _) => RiskLevel::Blocked,
            (RiskLevel::High, RiskLevel::Blocked) => RiskLevel::Blocked,
            (ur, _) => *ur,
        };
        return Ok(Json(RiskCheckResult { risk_level: final_risk, matched_rule: Some("user_rule".into()) }));
    }
    Ok(Json(RiskCheckResult { risk_level: base, matched_rule: None }))
}

// ── Exec Preview ────────────────────────────────────────────────────────────

async fn exec_preview(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<ExecPreviewBody>) -> Result<Json<ExecPlan>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;

    if let Some(hosts) = body.hosts {
        // Multi-host preview
        preview_exec_multi(hosts, &body.command, body.tags, body.timeout_secs)
            .await
            .map(Json)
            .map_err(|e| err(StatusCode::BAD_REQUEST, e))
    } else if let Some(host) = body.host {
        // Single-host preview
        preview_exec(&host, &body.command, body.timeout_secs)
            .await
            .map(Json)
            .map_err(|e| err(StatusCode::BAD_REQUEST, e))
    } else {
        Err(err(StatusCode::BAD_REQUEST, "host or hosts required"))
    }
}

// ── Connections ──────────────────────────────────────────────────────────────

async fn connection_status(State(s): State<AppState>, headers: HeaderMap) -> Result<Json<Vec<ConnectionStatus>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    Ok(Json(list_active_connections().await))
}
async fn ssh_connect(State(s): State<AppState>, headers: HeaderMap, Path(host): Path<String>) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    connect_host(&host).await.map(|_| Json(OkBody { ok: true })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn ssh_disconnect(State(s): State<AppState>, headers: HeaderMap, Path(host): Path<String>) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    disconnect_host(&host).await.map(|_| Json(OkBody { ok: true })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

// ── Webhook config ───────────────────────────────────────────────────────────

async fn get_webhook_config(
    State(s): State<AppState>, headers: HeaderMap,
) -> Result<Json<WebhookConfig>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    Ok(Json(load_webhook_config().unwrap_or_default()))
}

async fn set_webhook_config(
    State(s): State<AppState>, headers: HeaderMap, Json(config): Json<WebhookConfig>,
) -> Result<Json<WebhookConfig>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    save_webhook_config(&config).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(config))
}

// ── Playbooks ─────────────────────────────────────────────────────────────────

async fn list_playbooks(
    State(s): State<AppState>, headers: HeaderMap,
) -> Result<Json<Vec<Playbook>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    list_playbooks_core().map(Json).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn run_playbook(
    State(s): State<AppState>, headers: HeaderMap, Json(body): Json<PlaybookRunBody>,
) -> Result<Json<PlaybookRunResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    run_playbook_core(&body.playbook, &body.host, body.force, body.params.as_ref())
        .await
        .map(Json)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

async fn dry_run_playbook_handler(
    State(s): State<AppState>, headers: HeaderMap, Json(body): Json<PlaybookDryRunBody>,
) -> Result<Json<PlaybookDryRun>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let params = body.params.unwrap_or_default();
    dry_run_playbook(&body.playbook, &params)
        .map(Json)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

// ── Remote daemons ───────────────────────────────────────────────────────────

async fn list_daemons(
    State(s): State<AppState>, headers: HeaderMap,
) -> Result<Json<Vec<agent2ssh::remote::DaemonInfo>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    list_daemons_core().map(Json).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

// ── Daemon Diagnostics (F5-1) ───────────────────────────────────────────────

async fn diagnose_alias(
    State(s): State<AppState>, headers: HeaderMap, Path(alias): Path<String>,
) -> Result<Json<agent2ssh::remote::DaemonDiagnostic>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    diagnose_daemon(&alias)
        .await
        .map(Json)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

async fn diagnose_all(
    State(s): State<AppState>, headers: HeaderMap,
) -> Result<Json<Vec<agent2ssh::remote::DaemonDiagnostic>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let remotes = agent2ssh::remote::load_remotes()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let mut results = Vec::new();
    for remote in &remotes {
        match diagnose_daemon(&remote.alias).await {
            Ok(diag) => results.push(diag),
            Err(e) => {
                // Still include failed diagnostics for reporting
                results.push(agent2ssh::remote::DaemonDiagnostic {
                    alias: remote.alias.clone(),
                    url: remote.url.clone(),
                    checks: vec![agent2ssh::remote::DiagnosticCheck {
                        name: "diagnostic".to_string(),
                        status: agent2ssh::remote::DiagnosticStatus::Error,
                        message: format!("Failed to run diagnostic: {}", e),
                        details: None,
                    }],
                    overall_status: agent2ssh::remote::DiagnosticStatus::Error,
                });
            }
        }
    }
    Ok(Json(results))
}

// ── Daemon Version Check (F5-2) ─────────────────────────────────────────────

async fn version_check_alias(
    State(s): State<AppState>, headers: HeaderMap, Path(alias): Path<String>,
) -> Result<Json<agent2ssh::remote::VersionCompatibility>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    check_daemon_version(&alias)
        .await
        .map(Json)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

// ── Team Config Export/Import ───────────────────────────────────────────────

async fn config_export(
    State(s): State<AppState>, headers: HeaderMap,
) -> Result<Json<TeamConfigExport>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    export_team_config().map(Json).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn config_import(
    State(s): State<AppState>, headers: HeaderMap, Json(export): Json<TeamConfigExport>,
) -> Result<Json<ImportResult>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    import_team_config(&export).map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

async fn config_import_preview(
    State(s): State<AppState>, headers: HeaderMap, Json(export): Json<TeamConfigExport>,
) -> Result<Json<ConfigDiffPreview>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    preview_team_config_import(&export).map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

async fn proxy_exec(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(alias): Path<String>,
    Json(req): Json<ExecRequest>,
) -> Result<Json<ExecResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;

    // If alias is "localhost", execute locally
    if alias == "localhost" {
        // Apply same user-rule checks as local exec
        if let Some(user_risk) = classify_with_user_rules(&req.command).await {
            if user_risk == RiskLevel::Blocked {
                EXEC_BLOCKED_COUNT.fetch_add(1, Ordering::Relaxed);
                return Err(err(StatusCode::BAD_REQUEST, format!("command blocked by user rule: '{}'", req.command)));
            }
        }
        return exec_ssh_core(req).await.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e));
    }

    // Look up remote daemon
    let (url, remote_token) = get_daemon(&alias)
        .map_err(|e| err(StatusCode::NOT_FOUND, e))?;
    let token = remote_token
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, format!("no token configured for daemon '{}'", alias)))?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(req.timeout_secs.unwrap_or(60) + 10))
        .build()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let resp = client
        .post(format!("{}/exec", url.trim_end_matches('/')))
        .bearer_auth(&token)
        .json(&req)
        .send()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("remote exec failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(err(
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            body,
        ));
    }

    let result: ExecResult = resp.json().await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("invalid response from remote: {e}")))?;
    Ok(Json(result))
}

// ── Daemons Unified View (F5-4) ─────────────────────────────────────────────

async fn daemons_view(
    State(s): State<AppState>, headers: HeaderMap,
) -> Result<Json<agent2ssh::remote::DaemonUnifiedView>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    get_daemons_unified_view()
        .await
        .map(Json)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

// ── Metrics Trend (F6-3) ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TrendQuery {
    period: Option<String>,
}

async fn metrics_trend(
    State(s): State<AppState>, headers: HeaderMap, Query(q): Query<TrendQuery>,
) -> Result<Json<MetricsTrend>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let period = match q.period.as_deref().unwrap_or("24h") {
        "24h" | "last24h" => TrendPeriod::Last24h,
        "7d" | "last7d" => TrendPeriod::Last7d,
        "30d" | "last30d" => TrendPeriod::Last30d,
        "all" => TrendPeriod::All,
        other => return Err(err(StatusCode::BAD_REQUEST, format!("unknown period '{}'. Use: 24h, 7d, 30d, or all", other))),
    };
    compute_metrics_trend(period)
        .map(Json)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

// ── Event Stream SSE (F6-4) ─────────────────────────────────────────────────

async fn events_stream(
    State(s): State<AppState>, headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);

    let rx = subscribe_events();
    let rx = std::sync::Arc::new(tokio::sync::Mutex::new(rx));
    let stream = futures_util::stream::unfold(rx, move |rx| async move {
        let mut guard = rx.lock().await;
        match guard.recv().await {
            Ok(evt) => {
                let data = serde_json::to_string(&evt).unwrap_or_default();
                drop(guard);
                Some((Ok(Event::default().event("agent2ssh").data(data)), rx))
            }
            Err(_) => {
                None
            }
        }
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

// ── SSH config sync (F2-4) ───────────────────────────────────────────────────

async fn ssh_sync_diff(
    State(s): State<AppState>, headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let diff = agent2ssh::compare_ssh_configs(None)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("ssh sync diff failed: {e}")))?;
    Ok(Json(serde_json::to_value(diff).unwrap_or_default()))
}

async fn ssh_sync_export_handler(
    State(s): State<AppState>, headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let (path, count) = agent2ssh::export_to_ssh_config(None, None)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("ssh sync export failed: {e}")))?;
    Ok(Json(serde_json::json!({ "path": path, "hosts_exported": count })))
}

// ── WebSocket streaming exec ─────────────────────────────────────────────────

async fn exec_stream(
    State(s): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    // Fix-2: Authenticate before WebSocket upgrade
    if let Err(e) = check_auth(&s, &headers) {
        return e.into_response();
    }
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    ws.on_upgrade(|socket| async move {
        use axum::extract::ws::Message;
        use tokio::io::AsyncReadExt;
        use std::sync::Arc;

        let socket = Arc::new(tokio::sync::Mutex::new(socket));

        // Wait for ExecRequest message
        let req_msg = {
            let mut s = socket.lock().await;
            match s.recv().await {
                Some(Ok(Message::Text(text))) => text,
                _ => return,
            }
        };
        let req: ExecRequest = match serde_json::from_str(&req_msg) {
            Ok(r) => r,
            Err(e) => {
                let mut s = socket.lock().await;
                let _ = s.send(Message::Text(serde_json::json!({"type":"error","error":e.to_string()}).to_string())).await;
                return;
            }
        };

        let risk = classify_risk(&req.command);
        if risk == RiskLevel::Blocked || (risk == RiskLevel::High && !req.force) {
            let mut s = socket.lock().await;
            let _ = s.send(Message::Text(serde_json::json!({"type":"error","error":"blocked or force required"}).to_string())).await;
            return;
        }

        let host = match load_config().ok().and_then(|c| c.hosts.into_iter().find(|h| h.name == req.host)) {
            Some(h) => h,
            None => {
                let mut s = socket.lock().await;
                let _ = s.send(Message::Text(serde_json::json!({"type":"error","error":"unknown host"}).to_string())).await;
                return;
            }
        };

        let started = std::time::Instant::now();
        let timeout_secs = req.timeout_secs.unwrap_or(60);

        let mut cmd = tokio::process::Command::new("ssh");
        cmd.arg("-o").arg("BatchMode=yes").arg("-o").arg("StrictHostKeyChecking=accept-new")
           .arg("-p").arg(host.port.unwrap_or(22).to_string());
        if let Some(kp) = &host.key_path { if !kp.trim().is_empty() { cmd.arg("-i").arg(expand_tilde(kp)); } }
        // ProxyJump support
        if let Some(jump_name) = &host.jump_host {
            if let Some(jump) = load_config().ok().and_then(|c| c.hosts.into_iter().find(|h| h.name == *jump_name)) {
                let jump_target = match &jump.user {
                    Some(u) if !u.trim().is_empty() => format!("{}@{}:{}", u, jump.host, jump.port.unwrap_or(22)),
                    _ => format!("{}:{}", jump.host, jump.port.unwrap_or(22)),
                };
                cmd.arg("-J").arg(jump_target);
                if let Some(jkey) = &jump.key_path {
                    if !jkey.trim().is_empty() { cmd.arg("-i").arg(expand_tilde(jkey)); }
                }
            }
        }
        let target = match &host.user { Some(u) if !u.trim().is_empty() => format!("{}@{}", u, host.host), _ => host.host.clone() };
        cmd.arg(&target).arg(&req.command)
           .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).stdin(std::process::Stdio::null());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let mut s = socket.lock().await;
                let _ = s.send(Message::Text(serde_json::json!({"type":"error","error":e.to_string()}).to_string())).await;
                return;
            }
        };

        // Fix-3: Concurrently stream both stdout and stderr
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let sock_out = socket.clone();
        let stdout_task = tokio::spawn(async move {
            if let Some(stdout) = stdout {
                let mut reader = tokio::io::BufReader::new(stdout);
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let data = String::from_utf8_lossy(&buf[..n]).to_string();
                            let mut s = sock_out.lock().await;
                            if s.send(Message::Text(serde_json::json!({"type":"stdout","data":data}).to_string())).await.is_err() { break; }
                        }
                    }
                }
            }
        });

        let sock_err = socket.clone();
        let stderr_task = tokio::spawn(async move {
            if let Some(stderr) = stderr {
                let mut reader = tokio::io::BufReader::new(stderr);
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let data = String::from_utf8_lossy(&buf[..n]).to_string();
                            let mut s = sock_err.lock().await;
                            if s.send(Message::Text(serde_json::json!({"type":"stderr","data":data}).to_string())).await.is_err() { break; }
                        }
                    }
                }
            }
        });

        // Wait for child process with timeout
        let status = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            child.wait(),
        ).await;

        let _ = tokio::join!(stdout_task, stderr_task);

        let code = match status {
            Ok(Ok(s)) => s.code(),
            Ok(Err(_)) => None,
            Err(_) => {
                let _ = child.kill().await;
                None
            }
        };

        let mut s = socket.lock().await;
        let _ = s.send(Message::Text(
            serde_json::json!({"type":"exit","code":code,"duration_ms":started.elapsed().as_millis()}).to_string()
        )).await;
    })
}

fn expand_tilde(path: &str) -> String {
    if path == "~" { return dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_else(|| path.to_string()); }
    if let Some(rest) = path.strip_prefix("~/") { if let Some(home) = dirs::home_dir() { return home.join(rest).display().to_string(); } }
    path.to_string()
}

/// Check whether a binary exists on PATH (used by health + doctor).
pub fn which_binary(name: &str) -> Option<String> {
    let output = std::process::Command::new("which").arg(name).output().ok()?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() { return Some(path); }
    }
    None
}

// ── Health Snapshot ─────────────────────────────────────────────────────────

async fn get_health_snapshot(
    State(s): State<AppState>, headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    match load_health_snapshot() {
        Ok(snapshot) => Ok(Json(serde_json::to_value(snapshot).unwrap_or_default())),
        Err(_) => Ok(Json(serde_json::json!({
            "error": "no health snapshot available",
            "hint": "POST /health-snapshot to collect a fresh one"
        }))),
    }
}

async fn post_health_snapshot(
    State(s): State<AppState>, headers: HeaderMap, body: Option<Json<HealthSnapshotBody>>,
) -> Result<Json<agent2ssh::health::HealthSnapshot>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;

    let (hosts, timeout_secs) = match body {
        Some(Json(b)) => (b.hosts, b.timeout_secs),
        None => (None, None),
    };

    let target_hosts = match hosts {
        Some(h) if !h.is_empty() => h,
        _ => {
            // Collect health for ALL configured hosts
            load_config()
                .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?
                .hosts
                .iter()
                .map(|h| h.name.clone())
                .collect()
        }
    };

    let snapshot = collect_health_snapshot(target_hosts, timeout_secs).await;
    Ok(Json(snapshot))
}

// ── Web Console ──────────────────────────────────────────────────────────────

async fn serve_console() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("../../web/console.html"))
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── P9-1: Initialize structured logging ──────────────────────────────────
    let log_level = std::env::var("AGENT2SSH_LOG").unwrap_or_else(|_| "info".to_string());
    let log_format = std::env::var("AGENT2SSH_LOG_FORMAT").unwrap_or_else(|_| "text".to_string());

    let env_filter = tracing_subscriber::EnvFilter::try_new(&log_level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    match log_format.as_str() {
        "json" => {
            tracing_subscriber::fmt()
                .json()
                .with_env_filter(env_filter)
                .init();
        }
        _ => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .init();
        }
    }

    // Record start time for uptime calculation
    START_TIME.get_or_init(Instant::now);

    let addr = "127.0.0.1:7722";
    let config_dir = config_dir()?;
    std::fs::create_dir_all(&config_dir)?;

    // Token
    let token_path = config_dir.join("daemon.token");
    let token = if token_path.exists() {
        // Audit existing token file permissions before fixing
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&token_path) {
                let mode = meta.permissions().mode() & 0o777;
                if mode != 0o600 {
                    tracing::warn!(
                        mode = format!("{:o}", mode),
                        "daemon.token had overly permissive mode, fixing to 0600"
                    );
                }
            }
        }
        let existing = std::fs::read_to_string(&token_path)?.trim().to_string();
        restrict_file_to_owner(&token_path)?;
        existing
    } else {
        let t = Uuid::new_v4().to_string();
        std::fs::write(&token_path, &t)?;
        restrict_file_to_owner(&token_path)?;
        t
    };

    // PID
    let pid_path = config_dir.join("daemon.pid");
    std::fs::write(&pid_path, std::process::id().to_string())?;

    let state = AppState { token: token.clone() };

    let cors = CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any);

    let app = Router::new()
        .route("/", get(|| async { axum::response::Redirect::to("/console") }))
        .route("/console", get(serve_console))
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/metrics/trend", get(metrics_trend))
        .route("/hosts", get(list_hosts).post(add_host))
        .route("/hosts/import", post(import_config))
        .route("/hosts/:name", delete(remove_host))
        .route("/ping", post(ping))
        .route("/exec", post(exec))
        .route("/exec-multi", post(exec_multi))
        .route("/exec/compare", post(exec_compare))
        .route("/exec/stream", get(exec_stream))
        .route("/audit", get(audit))
        .route("/audit/export", get(audit_export))
        .route("/sftp/upload", post(sftp_upload))
        .route("/sftp/download", post(sftp_download))
        .route("/sftp/ls", post(sftp_ls))
        .route("/sftp/stat", post(sftp_stat))
        .route("/sftp/mkdir", post(sftp_mkdir))
        .route("/sessions", post(session_open).get(session_list))
        .route("/sessions/:id/write", post(session_write))
        .route("/sessions/:id/read", get(session_read))
        .route("/sessions/:id", delete(session_close))
        .route("/forwards", post(forward_add).get(forward_list))
        .route("/forwards/:id", delete(forward_remove))
        .route("/approvals", get(approvals_list))
        .route("/approvals/:id/approve", post(approval_approve))
        .route("/approvals/:id/reject", post(approval_reject))
        .route("/approval/:id/respond", post(approval_respond_generic))
        .route("/approval/policies", get(list_policies).put(put_policies))
        .route("/approval/check", post(approval_check))
        .route("/risk/check", post(risk_check))
        .route("/exec/preview", post(exec_preview))
        .route("/connections", get(connection_status))
        .route("/connections/:host/connect", post(ssh_connect))
        .route("/connections/:host/disconnect", post(ssh_disconnect))
        .route("/playbooks", get(list_playbooks))
        .route("/playbooks/run", post(run_playbook))
        .route("/playbooks/:name/dry-run", post(dry_run_playbook_handler))
        .route("/daemons", get(list_daemons))
        .route("/daemons/view", get(daemons_view))
        .route("/daemons/:alias/exec", post(proxy_exec))
        .route("/diagnostics", get(diagnose_all))
        .route("/diagnostics/:alias", get(diagnose_alias))
        .route("/version-check/:alias", get(version_check_alias))
        .route("/config/export", get(config_export))
        .route("/config/import", post(config_import))
        .route("/config/import/preview", post(config_import_preview))
        .route("/webhook/config", get(get_webhook_config).put(set_webhook_config))
        .route("/health-snapshot", get(get_health_snapshot).post(post_health_snapshot))
        .route("/events/stream", get(events_stream))
        .route("/ssh-sync/diff", get(ssh_sync_diff))
        .route("/ssh-sync/export", post(ssh_sync_export_handler))
        .layer(cors)
        .with_state(state);

    tracing::info!(addr = %addr, "Agent2SSH daemon listening");
    tracing::info!(url = %format!("http://{addr}/console"), "Web console available");
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    let _ = std::fs::remove_file(&pid_path);
    Ok(())
}
