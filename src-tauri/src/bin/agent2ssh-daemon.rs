use agent2ssh::approval::ApprovalRequest as ApprovalRequestType;
use agent2ssh::approval::{
    approval_action_url, build_approval_context, check_approval_required, list_approval_policies,
    save_approval_policies, ApprovalPolicy,
};
use agent2ssh::approval::{
    approval_list, approval_request_with_context, approval_request_with_ttl, approval_respond,
    approval_wait, ApprovalStatus,
};
use agent2ssh::connection::{connect_host, disconnect_host, list_active_connections};
use agent2ssh::core::*;
use agent2ssh::events::{publish_event, subscribe_events, EventType};
use agent2ssh::execution_control::{
    append_rejected_exec_audit, authorize_command_with_approval, command_authorization_target,
    effective_command_risk, expand_exec_targets, ApprovalOutcome, ApprovalPrompt,
    CommandAuthorizationError, CommandAuthorizationInput,
};
use agent2ssh::forward::*;
use agent2ssh::gate::{
    gate_blocks_source, load_execution_gate, save_execution_gate, ExecutionGateMode,
    ExecutionGateStatus,
};
use agent2ssh::health::{collect_health_snapshot, load_health_snapshot};
use agent2ssh::limits::{load_execution_limits, ExecutionLimitRejection, ExecutionLimiter};
use agent2ssh::notify::{
    fire_webhook, load_webhook_config, notify_approval_pending, save_webhook_config, WebhookConfig,
    WebhookEvent,
};
use agent2ssh::playbook::{
    dry_run_playbook, list_playbooks_core, run_playbook_core_with_source, Playbook, PlaybookDryRun,
    PlaybookRunResult,
};
use agent2ssh::remote::{
    check_daemon_scope, check_daemon_version, diagnose_daemon, get_daemon_with_scope,
    get_daemons_unified_view, list_daemons_core, load_scoped_daemon_tokens,
    resolve_scoped_daemon_token, DaemonScope,
};
use agent2ssh::risk_config::classify_with_user_rules;
use agent2ssh::session::*;
use agent2ssh::store::*;
use agent2ssh::types::*;

use axum::{
    extract::{Path, Query, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
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
    START_TIME.get_or_init(Instant::now).elapsed().as_secs()
}

// ── State ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    token: String,
    limiter: Arc<Mutex<ExecutionLimiter>>,
}

#[derive(Clone)]
struct AuthContext {
    scope: Option<DaemonScope>,
}

// ── Auth ─────────────────────────────────────────────────────────────────────

fn check_auth(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthContext, (StatusCode, Json<ErrorBody>)> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if token_matches(token, &state.token) {
        return Ok(AuthContext { scope: None });
    }

    let scoped_tokens = load_scoped_daemon_tokens().map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to load scoped daemon tokens: {e}"),
        )
    })?;
    for scoped in scoped_tokens {
        let Some(expected) = resolve_scoped_daemon_token(&scoped) else {
            continue;
        };
        if token_matches(token, &expected) {
            return Ok(AuthContext {
                scope: scoped.scope.clone(),
            });
        }
    }

    tracing::warn!("failed authentication attempt");
    Err((
        StatusCode::UNAUTHORIZED,
        Json(ErrorBody {
            error: "unauthorized".into(),
        }),
    ))
}

fn token_matches(candidate: &str, expected: &str) -> bool {
    if expected.trim().is_empty() {
        return false;
    }

    let candidate = candidate.as_bytes();
    let expected = expected.as_bytes();
    let mut diff = candidate.len() ^ expected.len();
    let max_len = candidate.len().max(expected.len());

    for i in 0..max_len {
        let candidate_byte = candidate.get(i).copied().unwrap_or(0);
        let expected_byte = expected.get(i).copied().unwrap_or(0);
        diff |= (candidate_byte ^ expected_byte) as usize;
    }

    diff == 0
}

fn load_or_create_daemon_token(config_dir: &std::path::Path) -> anyhow::Result<String> {
    let token_path = config_dir.join("daemon.token");
    if token_path.exists() {
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
        if !existing.is_empty() {
            restrict_file_to_owner(&token_path)?;
            return Ok(existing);
        }
        tracing::warn!("daemon.token was empty, generating a new token");
    }

    let token = Uuid::new_v4().to_string();
    std::fs::write(&token_path, &token)?;
    restrict_file_to_owner(&token_path)?;
    Ok(token)
}

// ── Error type ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
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

fn locked_status() -> StatusCode {
    StatusCode::from_u16(423).expect("423 is a valid HTTP status")
}

fn too_many_requests_status() -> StatusCode {
    StatusCode::from_u16(429).expect("429 is a valid HTTP status")
}

fn source_or_env(source: Option<String>, default_source: &str) -> String {
    source
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| source_from_env(default_source))
}

fn reject_if_gate_paused(
    source: &str,
    host: &str,
    command: &str,
) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    let status = load_execution_gate().map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to load execution gate: {e}"),
        )
    })?;
    if !gate_blocks_source(&status, source) {
        return Ok(());
    }

    let result = ExecResult {
        host: host.to_string(),
        command: command.to_string(),
        exit_code: None,
        stdout: String::new(),
        stderr: "execution gate paused".to_string(),
        duration_ms: 0,
        risk_level: RiskLevel::Blocked,
        truncated: false,
    };
    let _ = append_audit(
        &result,
        RiskLevel::Blocked,
        status.reason.as_deref().or(Some("execution gate paused")),
        None,
        Some(source),
    );
    publish_event(
        EventType::GateRejected,
        serde_json::json!({
            "source": source,
            "host": host,
            "command_preview": preview_text(command, 2048),
            "mode": status.mode.to_string(),
            "reason": status.reason,
        }),
    );
    Err(err(locked_status(), "execution gate paused"))
}

fn host_tags(host: &str) -> Vec<String> {
    command_authorization_target(host).tags
}

fn host_risk_override(host: &str) -> Option<RiskLevel> {
    command_authorization_target(host).risk_override
}

async fn request_and_wait_for_approval(prompt: ApprovalPrompt) -> Result<ApprovalOutcome, String> {
    APPROVAL_COUNT.fetch_add(1, Ordering::Relaxed);
    let approval_ctx = build_approval_context(&prompt.host, &prompt.command, &prompt.source)
        .ok()
        .map(|mut ctx| {
            ctx.reason = prompt.reason.clone();
            ctx.change_id = prompt.change_id.clone();
            ctx
        });
    let approval_id = if let Some(ctx) = approval_ctx {
        approval_request_with_context(
            &prompt.host,
            &prompt.command,
            prompt.risk,
            prompt.ttl_secs,
            ctx,
        )
        .await
    } else {
        approval_request_with_ttl(&prompt.host, &prompt.command, prompt.risk, prompt.ttl_secs).await
    };

    let host_notify = prompt.host.clone();
    let cmd_notify = prompt.command.clone();
    let approval_id_notify = approval_id.to_string();
    let risk_notify = format!("{:?}", prompt.risk).to_lowercase();
    let action_url = approval_action_url("http://127.0.0.1:7722", &approval_id_notify);
    let evt = WebhookEvent {
        event: "approval_required".into(),
        host: host_notify.clone(),
        command: cmd_notify.clone(),
        approval_id: Some(approval_id_notify.clone()),
        risk_level: Some(risk_notify.clone()),
        exit_code: None,
    };
    tokio::spawn(async move {
        if let Err(e) = notify_approval_pending(
            &approval_id_notify,
            &host_notify,
            &cmd_notify,
            &risk_notify,
            Some(&action_url),
        )
        .await
        {
            tracing::error!(error = %e, "approval notification error");
        }
        if let Err(e) = fire_webhook(evt).await {
            tracing::error!(error = %e, "webhook fire error");
        }
    });

    match approval_wait(approval_id).await {
        ApprovalStatus::Approved => Ok(ApprovalOutcome::Approved),
        ApprovalStatus::Rejected => Ok(ApprovalOutcome::Rejected),
        ApprovalStatus::TimedOut => Ok(ApprovalOutcome::TimedOut),
        _ => Err("unexpected approval status".to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
async fn authorize_command(
    auth_scope: &Option<DaemonScope>,
    source: &str,
    host: &str,
    tags: &[String],
    risk_override: Option<RiskLevel>,
    command: &str,
    force: bool,
    reason: Option<String>,
    change_id: Option<String>,
) -> Result<(RiskLevel, bool), (StatusCode, Json<ErrorBody>)> {
    let input = CommandAuthorizationInput {
        auth_scope,
        source,
        host,
        tags,
        risk_override: risk_override.or_else(|| host_risk_override(host)),
        command,
        force,
        reason,
        change_id,
    };

    match authorize_command_with_approval(input, request_and_wait_for_approval).await {
        Ok(result) => Ok((result.risk, result.approved)),
        Err(CommandAuthorizationError::ScopeDenied(message)) => {
            Err(err(StatusCode::FORBIDDEN, message))
        }
        Err(CommandAuthorizationError::Blocked { risk, message }) => {
            EXEC_BLOCKED_COUNT.fetch_add(1, Ordering::Relaxed);
            let evt = WebhookEvent {
                event: "exec_blocked".into(),
                host: host.to_string(),
                command: command.to_string(),
                approval_id: None,
                risk_level: Some(format!("{risk}")),
                exit_code: None,
            };
            tokio::spawn(async move {
                if let Err(e) = fire_webhook(evt).await {
                    tracing::error!(error = %e, "webhook fire error");
                }
            });
            Err(err(StatusCode::BAD_REQUEST, message))
        }
        Err(CommandAuthorizationError::ApprovalRejected) => {
            Err(err(StatusCode::FORBIDDEN, "command rejected by approver"))
        }
        Err(CommandAuthorizationError::ApprovalTimedOut) => Err(err(
            StatusCode::REQUEST_TIMEOUT,
            "approval request timed out",
        )),
        Err(CommandAuthorizationError::Internal(message)) => {
            Err(err(StatusCode::INTERNAL_SERVER_ERROR, message))
        }
    }
}

fn targets_for_exec_multi(
    hosts: &[String],
    tags: &Option<Vec<String>>,
) -> Vec<(String, Vec<String>)> {
    if let Ok(targets) = expand_exec_targets(hosts, tags) {
        if !targets.is_empty() {
            return targets;
        }
    }

    if hosts.is_empty() {
        return tags
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|tag| (format!("tag:{tag}"), vec![tag]))
            .collect();
    }
    let request_tags = tags.clone().unwrap_or_default();
    hosts
        .iter()
        .map(|host| {
            let mut all_tags = host_tags(host);
            for tag in &request_tags {
                if !all_tags.iter().any(|existing| existing == tag) {
                    all_tags.push(tag.clone());
                }
            }
            (host.clone(), all_tags)
        })
        .collect()
}

fn target_host_label(hosts: &[String], tags: &Option<Vec<String>>) -> String {
    if !hosts.is_empty() {
        return hosts.join(",");
    }
    let tag_label = tags
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter(|tag| !tag.trim().is_empty())
        .map(|tag| format!("tag:{tag}"))
        .collect::<Vec<_>>()
        .join(",");
    if tag_label.is_empty() {
        "no-targets".to_string()
    } else {
        tag_label
    }
}

#[allow(clippy::too_many_arguments)]
async fn authorize_targets(
    auth_scope: &Option<DaemonScope>,
    source: &str,
    targets: &[(String, Vec<String>)],
    fallback_host: &str,
    command: &str,
    force: bool,
    reason: Option<String>,
    change_id: Option<String>,
) -> Result<bool, (StatusCode, Json<ErrorBody>)> {
    let mut high_risk_approved = false;
    if targets.is_empty() {
        let (risk, approved) = authorize_command(
            auth_scope,
            source,
            fallback_host,
            &[],
            None,
            command,
            force,
            reason,
            change_id,
        )
        .await?;
        return Ok(approved && risk == RiskLevel::High);
    }

    for (host, tags) in targets {
        let (risk, approved) = authorize_command(
            auth_scope,
            source,
            host,
            tags,
            None,
            command,
            force || high_risk_approved,
            reason.clone(),
            change_id.clone(),
        )
        .await?;
        if approved && risk == RiskLevel::High {
            high_risk_approved = true;
        }
    }
    Ok(high_risk_approved)
}

fn write_limit_rejection_audit(
    rejection: &ExecutionLimitRejection,
    source: &str,
    host: &str,
    command: &str,
) {
    let result = ExecResult {
        host: host.to_string(),
        command: command.to_string(),
        exit_code: None,
        stdout: String::new(),
        stderr: format!(
            "execution limit exceeded: {} current={} limit={}",
            rejection.scope, rejection.current, rejection.limit
        ),
        duration_ms: 0,
        risk_level: RiskLevel::Blocked,
        truncated: false,
    };
    let reason = format!(
        "execution limit exceeded: {} current={} limit={}",
        rejection.scope, rejection.current, rejection.limit
    );
    let _ = append_audit(
        &result,
        RiskLevel::Blocked,
        Some(&reason),
        None,
        Some(source),
    );
    publish_event(
        EventType::LimitRejected,
        serde_json::json!({
            "source": source,
            "host": host,
            "command_preview": preview_text(command, 2048),
            "scope": rejection.scope,
            "current": rejection.current,
            "limit": rejection.limit,
        }),
    );
}

async fn reject_if_rate_limited(
    state: &AppState,
    source: &str,
    targets: &[(String, Vec<String>)],
    command: &str,
) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    let mut limiter = state.limiter.lock().await;
    if let Err(rejection) = limiter.check_execution_batch(source, targets) {
        let host = targets
            .iter()
            .map(|(host, _)| host.as_str())
            .collect::<Vec<_>>()
            .join(",");
        write_limit_rejection_audit(&rejection, source, &host, command);
        return Err(err(
            too_many_requests_status(),
            format!(
                "execution limit exceeded: {} current={} limit={}",
                rejection.scope, rejection.current, rejection.limit
            ),
        ));
    }
    Ok(())
}

async fn reject_if_session_limited(
    state: &AppState,
    source: &str,
    host: &str,
    tags: &[String],
) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    let limiter = state.limiter.lock().await;
    if let Err(rejection) = limiter.check_session_open(source, host, tags) {
        write_limit_rejection_audit(&rejection, source, host, "session_open");
        return Err(err(
            too_many_requests_status(),
            format!(
                "execution limit exceeded: {} current={} limit={}",
                rejection.scope, rejection.current, rejection.limit
            ),
        ));
    }
    Ok(())
}

// ── Request/Response types ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct PingBody {
    hosts: Vec<String>,
    timeout_secs: Option<u64>,
}
#[derive(Deserialize)]
struct ExecMultiBody {
    hosts: Vec<String>,
    command: String,
    #[serde(default)]
    force: bool,
    timeout_secs: Option<u64>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    strategy: Option<BatchStrategy>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    change_id: Option<String>,
    #[serde(default)]
    source: Option<String>,
}
#[derive(Deserialize)]
struct ExecCompareBody {
    hosts: Vec<String>,
    command: String,
    #[serde(default)]
    force: bool,
    timeout_secs: Option<u64>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}
#[derive(Deserialize)]
struct SftpDirBody {
    host: String,
    path: String,
}
#[derive(Deserialize)]
struct SessionOpenBody {
    host: String,
    #[serde(default)]
    source: Option<String>,
}
#[derive(Deserialize)]
struct SessionWriteBody {
    input: String,
    #[serde(default)]
    source: Option<String>,
}
#[derive(Deserialize)]
struct ReadQuery {
    timeout_ms: Option<u64>,
    #[serde(default)]
    source: Option<String>,
}
#[derive(Deserialize)]
struct SourceQuery {
    #[serde(default)]
    source: Option<String>,
}
#[derive(Deserialize)]
struct AuditQuery {
    host: Option<String>,
    risk_level: Option<RiskLevel>,
    exit_code: Option<i32>,
    since: Option<String>,
    until: Option<String>,
    limit: Option<usize>,
    search: Option<String>,
    command_pattern: Option<String>,
    host_env: Option<String>,
    host_role: Option<String>,
    host_owner: Option<String>,
}
#[derive(Deserialize)]
struct AuditExportQuery {
    host: Option<String>,
    risk_level: Option<RiskLevel>,
    exit_code: Option<i32>,
    since: Option<String>,
    until: Option<String>,
    limit: Option<usize>,
    search: Option<String>,
    command_pattern: Option<String>,
    host_env: Option<String>,
    host_role: Option<String>,
    host_owner: Option<String>,
    format: Option<String>,
}
#[derive(Deserialize)]
struct RiskCheckBody {
    command: String,
    #[allow(dead_code)]
    host: Option<String>,
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
    #[serde(default)]
    source: Option<String>,
}
#[derive(Deserialize)]
struct PlaybookDryRunBody {
    playbook: String,
    #[serde(default)]
    params: Option<HashMap<String, String>>,
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
#[derive(Deserialize, Default)]
struct HealthSnapshotBody {
    #[serde(default)]
    hosts: Option<Vec<String>>,
    timeout_secs: Option<u64>,
}
#[derive(Deserialize)]
struct GateUpdateBody {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}
#[derive(Serialize)]
struct RiskCheckResult {
    risk_level: RiskLevel,
    matched_rule: Option<String>,
}
#[derive(Serialize)]
struct OkBody {
    ok: bool,
}
#[derive(Serialize)]
struct IdBody {
    id: String,
}
#[derive(Serialize)]
struct SessionListItem {
    id: String,
    host: String,
}
#[derive(Serialize)]
struct OutputBody {
    output: String,
}

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

async fn gate_status(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ExecutionGateStatus>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    load_execution_gate().map(Json).map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to load execution gate: {e}"),
        )
    })
}

async fn gate_pause(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<GateUpdateBody>,
) -> Result<Json<ExecutionGateStatus>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let source = source_or_env(body.source, "daemon");
    let status = save_execution_gate(ExecutionGateMode::Paused, Some(source.clone()), body.reason)
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to pause execution gate: {e}"),
            )
        })?;
    publish_event(
        EventType::GateChanged,
        serde_json::json!({
            "source": source,
            "mode": status.mode.to_string(),
            "reason": status.reason,
            "updated_at": status.updated_at,
        }),
    );
    Ok(Json(status))
}

async fn gate_resume(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<GateUpdateBody>,
) -> Result<Json<ExecutionGateStatus>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let source = source_or_env(body.source, "daemon");
    let status = save_execution_gate(ExecutionGateMode::Active, Some(source.clone()), body.reason)
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to resume execution gate: {e}"),
            )
        })?;
    publish_event(
        EventType::GateChanged,
        serde_json::json!({
            "source": source,
            "mode": status.mode.to_string(),
            "reason": status.reason,
            "updated_at": status.updated_at,
        }),
    );
    Ok(Json(status))
}

async fn list_hosts(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<HostProfile>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    list_hosts_core()
        .map(Json)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn add_host(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(host): Json<HostProfile>,
) -> Result<Json<HostProfile>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    add_host_core(host)
        .map(Json)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

async fn remove_host(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    remove_host_core(&name)
        .map(|_| Json(OkBody { ok: true }))
        .map_err(|e| err(StatusCode::NOT_FOUND, e))
}

async fn import_config(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<HostProfile>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    import_ssh_config_core(None)
        .map(Json)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn ping(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PingBody>,
) -> Result<Json<Vec<PingResult>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    Ok(Json(ping_hosts_core(body.hosts, body.timeout_secs).await))
}

async fn exec(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(mut req): Json<ExecRequest>,
) -> Result<Json<ExecResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    if req.source.is_none() {
        req.source = Some(source_from_env("daemon"));
    }
    let source = req.source.as_deref().unwrap_or("daemon").to_string();
    reject_if_gate_paused(&source, &req.host, &req.command)?;
    let targets = vec![(req.host.clone(), host_tags(&req.host))];
    reject_if_rate_limited(&s, &source, &targets, &req.command).await?;
    tracing::info!(host = %req.host, command = %req.command, "exec handler invoked");

    let exec_start = Instant::now();

    let tags = targets
        .first()
        .map(|(_, tags)| tags.clone())
        .unwrap_or_default();
    let (risk, approved) = authorize_command(
        &auth.scope,
        &source,
        &req.host,
        &tags,
        None,
        &req.command,
        req.force,
        req.reason.clone(),
        req.change_id.clone(),
    )
    .await?;
    if approved && risk == RiskLevel::High {
        req.force = true;
    }

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
    result
        .map(Json)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

async fn exec_multi(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ExecMultiBody>,
) -> Result<Json<ExecMultiBatchResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    tracing::info!(hosts = ?body.hosts, command = %body.command, "exec-multi handler invoked");
    let source = source_or_env(body.source, "daemon");
    let targets = targets_for_exec_multi(&body.hosts, &body.tags);
    let target_label = target_host_label(&body.hosts, &body.tags);
    reject_if_gate_paused(&source, &target_label, &body.command)?;
    reject_if_rate_limited(&s, &source, &targets, &body.command).await?;
    let mut force = body.force;
    if authorize_targets(
        &auth.scope,
        &source,
        &targets,
        &target_label,
        &body.command,
        force,
        body.reason.clone(),
        body.change_id.clone(),
    )
    .await?
    {
        force = true;
    }
    Ok(Json(
        exec_multi_with_strategy(ExecMultiBatchRequest {
            request: ExecMultiRequest {
                hosts: body.hosts,
                command: body.command,
                force,
                timeout_secs: body.timeout_secs,
                tags: body.tags,
                reason: body.reason,
                change_id: body.change_id,
                source: Some(source),
            },
            strategy: body.strategy,
        })
        .await,
    ))
}

async fn exec_compare(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ExecCompareBody>,
) -> Result<Json<ExecComparison>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    tracing::info!(hosts = ?body.hosts, command = %body.command, "exec-compare handler invoked");
    let source = source_from_env("daemon");
    let targets = targets_for_exec_multi(&body.hosts, &body.tags);
    let target_label = target_host_label(&body.hosts, &body.tags);
    reject_if_gate_paused(&source, &target_label, &body.command)?;
    reject_if_rate_limited(&s, &source, &targets, &body.command).await?;
    let mut force = body.force;
    if authorize_targets(
        &auth.scope,
        &source,
        &targets,
        &target_label,
        &body.command,
        force,
        None,
        None,
    )
    .await?
    {
        force = true;
    }
    let results = exec_multi_core(ExecMultiRequest {
        hosts: body.hosts,
        command: body.command,
        force,
        timeout_secs: body.timeout_secs,
        tags: body.tags,
        reason: None,
        change_id: None,
        source: Some(source),
    })
    .await;
    Ok(Json(compare_exec_results(&results)))
}

async fn audit(
    State(s): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEntry>>, (StatusCode, Json<ErrorBody>)> {
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
    list_audit_core(filter)
        .map(Json)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

// ── Audit Export (F6-2) ─────────────────────────────────────────────────────

async fn audit_export(
    State(s): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<AuditExportQuery>,
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
            let data =
                export_audit_csv(&filter).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
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

async fn sftp_upload(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SftpUploadRequest>,
) -> Result<Json<SftpResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    let source = source_from_env("daemon");
    let command = format!("sftp upload {} -> {}", req.local_path, req.remote_path);
    reject_if_gate_paused(&source, &req.host, &command)?;
    let targets = vec![(req.host.clone(), host_tags(&req.host))];
    reject_if_rate_limited(&s, &source, &targets, &command).await?;
    authorize_command(
        &auth.scope,
        &source,
        &req.host,
        &targets
            .first()
            .map(|(_, tags)| tags.clone())
            .unwrap_or_default(),
        None,
        &command,
        true,
        None,
        None,
    )
    .await?;
    sftp_upload_core_with_source(req, Some(source))
        .await
        .map(Json)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn sftp_download(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SftpDownloadRequest>,
) -> Result<Json<SftpResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    let source = source_from_env("daemon");
    let command = format!("sftp download {} -> {}", req.remote_path, req.local_path);
    reject_if_gate_paused(&source, &req.host, &command)?;
    let targets = vec![(req.host.clone(), host_tags(&req.host))];
    reject_if_rate_limited(&s, &source, &targets, &command).await?;
    authorize_command(
        &auth.scope,
        &source,
        &req.host,
        &targets
            .first()
            .map(|(_, tags)| tags.clone())
            .unwrap_or_default(),
        None,
        &command,
        true,
        None,
        None,
    )
    .await?;
    sftp_download_core_with_source(req, Some(source))
        .await
        .map(Json)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn sftp_ls(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SftpDirBody>,
) -> Result<Json<ExecResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    let source = source_from_env("daemon");
    let command = format!("sftp ls {}", body.path);
    reject_if_gate_paused(&source, &body.host, &command)?;
    let targets = vec![(body.host.clone(), host_tags(&body.host))];
    reject_if_rate_limited(&s, &source, &targets, &command).await?;
    authorize_command(
        &auth.scope,
        &source,
        &body.host,
        &targets
            .first()
            .map(|(_, tags)| tags.clone())
            .unwrap_or_default(),
        None,
        &command,
        true,
        None,
        None,
    )
    .await?;
    sftp_ls_core_with_source(&body.host, &body.path, None, Some(source))
        .await
        .map(Json)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn sftp_stat(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SftpDirBody>,
) -> Result<Json<ExecResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    let source = source_from_env("daemon");
    let command = format!("sftp stat {}", body.path);
    reject_if_gate_paused(&source, &body.host, &command)?;
    let targets = vec![(body.host.clone(), host_tags(&body.host))];
    reject_if_rate_limited(&s, &source, &targets, &command).await?;
    authorize_command(
        &auth.scope,
        &source,
        &body.host,
        &targets
            .first()
            .map(|(_, tags)| tags.clone())
            .unwrap_or_default(),
        None,
        &command,
        true,
        None,
        None,
    )
    .await?;
    sftp_stat_core_with_source(&body.host, &body.path, None, Some(source))
        .await
        .map(Json)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn sftp_mkdir(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SftpDirBody>,
) -> Result<Json<ExecResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    let source = source_from_env("daemon");
    let command = format!("sftp mkdir {}", body.path);
    reject_if_gate_paused(&source, &body.host, &command)?;
    let targets = vec![(body.host.clone(), host_tags(&body.host))];
    reject_if_rate_limited(&s, &source, &targets, &command).await?;
    authorize_command(
        &auth.scope,
        &source,
        &body.host,
        &targets
            .first()
            .map(|(_, tags)| tags.clone())
            .unwrap_or_default(),
        None,
        &command,
        true,
        None,
        None,
    )
    .await?;
    sftp_mkdir_core_with_source(&body.host, &body.path, None, Some(source))
        .await
        .map(Json)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

// ── Sessions ─────────────────────────────────────────────────────────────────

async fn session_open(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SessionOpenBody>,
) -> Result<Json<IdBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    tracing::info!(host = %body.host, "session_open invoked");
    let source = body.source.as_deref().unwrap_or("daemon");
    let tags = host_tags(&body.host);
    reject_if_gate_paused(source, &body.host, "session_open")?;
    check_daemon_scope(&auth.scope, &body.host, &tags, "session_open")
        .map_err(|e| err(StatusCode::FORBIDDEN, e))?;
    reject_if_session_limited(&s, source, &body.host, &tags).await?;
    authorize_command(
        &auth.scope,
        source,
        &body.host,
        &tags,
        None,
        "session_open",
        true,
        None,
        None,
    )
    .await?;
    match session_open_core(&body.host).await {
        Ok(id) => {
            s.limiter
                .lock()
                .await
                .register_session(id, source, &body.host, &tags);
            publish_event(
                EventType::SessionOpened,
                serde_json::json!({
                    "source": source,
                    "host": body.host,
                    "session_id": id.to_string(),
                }),
            );
            Ok(Json(IdBody { id: id.to_string() }))
        }
        Err(e) => Err(err(StatusCode::BAD_REQUEST, e)),
    }
}
async fn session_write(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SessionWriteBody>,
) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    let source = body.source.as_deref().unwrap_or("daemon");
    reject_if_gate_paused(source, &format!("session:{id}"), &body.input)?;
    let targets = s
        .limiter
        .lock()
        .await
        .session_target(&uuid)
        .map(|target| vec![target])
        .unwrap_or_else(|| vec![(format!("session:{id}"), Vec::new())]);
    reject_if_rate_limited(&s, source, &targets, &body.input).await?;
    let session_host = targets
        .first()
        .map(|(host, _)| host.clone())
        .unwrap_or_else(|| format!("session:{id}"));
    let session_tags = targets
        .first()
        .map(|(_, tags)| tags.clone())
        .unwrap_or_default();
    authorize_command(
        &auth.scope,
        source,
        &session_host,
        &session_tags,
        None,
        &body.input,
        false,
        None,
        None,
    )
    .await?;
    session_write_core(uuid, &body.input)
        .await
        .map(|_| {
            publish_event(
                EventType::SessionInput,
                serde_json::json!({
                    "source": source,
                    "session_id": id,
                    "input_preview": preview_text(&body.input, 2048),
                    "input_bytes": body.input.len(),
                }),
            );
            Json(OkBody { ok: true })
        })
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn session_read(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ReadQuery>,
) -> Result<Json<OutputBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    let source = q.source.as_deref().unwrap_or("daemon");
    let (session_host, session_tags) = s
        .limiter
        .lock()
        .await
        .session_target(&uuid)
        .unwrap_or_else(|| (format!("session:{id}"), Vec::new()));
    check_daemon_scope(&auth.scope, &session_host, &session_tags, "session_read")
        .map_err(|e| err(StatusCode::FORBIDDEN, e))?;
    session_read_core(uuid, q.timeout_ms.unwrap_or(2000))
        .await
        .map(|output| {
            if !output.is_empty() {
                publish_event(
                    EventType::SessionOutput,
                    serde_json::json!({
                        "source": source,
                        "session_id": id,
                        "output_preview": preview_text(&output, 4096),
                        "output_bytes": output.len(),
                    }),
                );
            }
            Json(OutputBody { output })
        })
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn session_close(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<SourceQuery>,
) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    let source = q.source.as_deref().unwrap_or("daemon");
    let (session_host, session_tags) = s
        .limiter
        .lock()
        .await
        .session_target(&uuid)
        .unwrap_or_else(|| (format!("session:{id}"), Vec::new()));
    check_daemon_scope(&auth.scope, &session_host, &session_tags, "session_close")
        .map_err(|e| err(StatusCode::FORBIDDEN, e))?;
    session_close_core(uuid)
        .await
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    s.limiter.lock().await.unregister_session(&uuid);
    publish_event(
        EventType::SessionClosed,
        serde_json::json!({
            "source": source,
            "session_id": id,
        }),
    );
    Ok(Json(OkBody { ok: true }))
}
async fn session_list(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<SessionListItem>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    Ok(Json(
        session_list_core()
            .await
            .into_iter()
            .map(|(id, host)| SessionListItem {
                id: id.to_string(),
                host,
            })
            .collect(),
    ))
}

// ── Forwards ─────────────────────────────────────────────────────────────────

async fn forward_add(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ForwardRule>,
) -> Result<Json<ForwardRule>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    let source = source_from_env("daemon");
    let command = format!(
        "forward {} {}:{} -> {}:{}",
        req.direction, req.bind_port, req.target_host, req.host, req.target_port
    );
    reject_if_gate_paused(&source, &req.host, &command)?;
    let targets = vec![(req.host.clone(), host_tags(&req.host))];
    reject_if_rate_limited(&s, &source, &targets, &command).await?;
    authorize_command(
        &auth.scope,
        &source,
        &req.host,
        &targets
            .first()
            .map(|(_, tags)| tags.clone())
            .unwrap_or_default(),
        None,
        &command,
        true,
        None,
        None,
    )
    .await?;
    forward_add_core(
        &req.host,
        req.direction,
        req.bind_port,
        &req.target_host,
        req.target_port,
    )
    .await
    .map(Json)
    .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn forward_list(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ForwardRule>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    Ok(Json(forward_list_core().await))
}
async fn forward_remove(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    if let Some(rule) = forward_list_core()
        .await
        .into_iter()
        .find(|rule| rule.id == uuid)
    {
        let command = format!(
            "forward remove {} {}:{} -> {}:{}",
            rule.direction, rule.bind_port, rule.target_host, rule.host, rule.target_port
        );
        let tags = host_tags(&rule.host);
        check_daemon_scope(&auth.scope, &rule.host, &tags, &command)
            .map_err(|e| err(StatusCode::FORBIDDEN, e))?;
    }
    forward_remove_core(uuid)
        .await
        .map(|_| Json(OkBody { ok: true }))
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

// ── Approvals ────────────────────────────────────────────────────────────────

async fn approvals_list(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ApprovalRequestType>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    Ok(Json(approval_list().await))
}
async fn approval_approve(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
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
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    approval_respond(uuid, false)
        .await
        .map(|_| Json(OkBody { ok: true }))
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

#[derive(Deserialize)]
struct ApprovalRespondBody {
    approved: bool,
}

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
    approval_respond(uuid, body.approved)
        .await
        .map(|_| Json(OkBody { ok: true }))
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

// ── Approval policies ────────────────────────────────────────────────────────

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
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
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
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
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
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
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

// ── Risk check ───────────────────────────────────────────────────────────────

async fn risk_check(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RiskCheckBody>,
) -> Result<Json<RiskCheckResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let host_override = body
        .host
        .as_deref()
        .and_then(|host| command_authorization_target(host).risk_override);
    let matched_rule = classify_with_user_rules(&body.command)
        .await
        .map(|_| "user_rule".to_string());
    let risk = apply_risk_override(effective_command_risk(&body.command).await, host_override);
    Ok(Json(RiskCheckResult {
        risk_level: risk,
        matched_rule,
    }))
}

// ── Exec Preview ────────────────────────────────────────────────────────────

async fn exec_preview(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ExecPreviewBody>,
) -> Result<Json<ExecPlan>, (StatusCode, Json<ErrorBody>)> {
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

async fn connection_status(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ConnectionStatus>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    Ok(Json(list_active_connections().await))
}
async fn ssh_connect(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(host): Path<String>,
) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    let source = source_from_env("daemon");
    let tags = host_tags(&host);
    reject_if_gate_paused(&source, &host, "connect")?;
    check_daemon_scope(&auth.scope, &host, &tags, "connect")
        .map_err(|e| err(StatusCode::FORBIDDEN, e))?;
    authorize_command(
        &auth.scope,
        &source,
        &host,
        &tags,
        None,
        "connect",
        true,
        None,
        None,
    )
    .await?;
    connect_host(&host)
        .await
        .map(|_| Json(OkBody { ok: true }))
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn ssh_disconnect(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(host): Path<String>,
) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    let source = source_from_env("daemon");
    let tags = host_tags(&host);
    check_daemon_scope(&auth.scope, &host, &tags, "disconnect")
        .map_err(|e| err(StatusCode::FORBIDDEN, e))?;
    authorize_command(
        &auth.scope,
        &source,
        &host,
        &tags,
        None,
        "disconnect",
        true,
        None,
        None,
    )
    .await?;
    disconnect_host(&host)
        .await
        .map(|_| Json(OkBody { ok: true }))
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

// ── Webhook config ───────────────────────────────────────────────────────────

async fn get_webhook_config(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WebhookConfig>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    Ok(Json(load_webhook_config().unwrap_or_default()))
}

async fn set_webhook_config(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(config): Json<WebhookConfig>,
) -> Result<Json<WebhookConfig>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    save_webhook_config(&config).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(config))
}

// ── Playbooks ─────────────────────────────────────────────────────────────────

async fn list_playbooks(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Playbook>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
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
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    let source = source_or_env(body.source, "daemon");
    reject_if_gate_paused(&source, &body.host, &format!("playbook:{}", body.playbook))?;
    let targets = vec![(body.host.clone(), host_tags(&body.host))];
    reject_if_rate_limited(
        &s,
        &source,
        &targets,
        &format!("playbook:{}", body.playbook),
    )
    .await?;
    let params_for_dry_run = body.params.clone().unwrap_or_default();
    let dry_run = dry_run_playbook(&body.playbook, &params_for_dry_run)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    let playbook_risk_override = list_playbooks_core().ok().and_then(|playbooks| {
        playbooks
            .into_iter()
            .find(|playbook| playbook.name == body.playbook)
            .and_then(|playbook| playbook.risk_override)
    });
    let mut force = body.force;
    let tags = targets
        .first()
        .map(|(_, tags)| tags.clone())
        .unwrap_or_default();
    for step in &dry_run.steps {
        let base_risk = effective_command_risk(&step.command_resolved).await;
        let step_risk = apply_risk_override(base_risk, playbook_risk_override);
        if step_risk == RiskLevel::Blocked {
            append_rejected_exec_audit(
                &source,
                &body.host,
                &step.command_resolved,
                step_risk,
                "playbook step blocked by risk policy",
                body.change_id.as_deref(),
            );
            return Err(err(
                StatusCode::BAD_REQUEST,
                format!(
                    "playbook step blocked by risk policy: '{}'",
                    step.command_resolved
                ),
            ));
        }
        let (risk, approved) = authorize_command(
            &auth.scope,
            &source,
            &body.host,
            &tags,
            playbook_risk_override,
            &step.command_resolved,
            force,
            body.reason.clone(),
            body.change_id.clone(),
        )
        .await?;
        if approved && risk == RiskLevel::High {
            force = true;
        }
    }
    run_playbook_core_with_source(
        &body.playbook,
        &body.host,
        force,
        body.params.as_ref(),
        body.reason,
        body.change_id,
        Some(source),
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
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let params = body.params.unwrap_or_default();
    dry_run_playbook(&body.playbook, &params)
        .map(Json)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

// ── Remote daemons ───────────────────────────────────────────────────────────

async fn list_daemons(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<agent2ssh::remote::DaemonInfo>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    list_daemons_core()
        .map(Json)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

// ── Daemon Diagnostics (F5-1) ───────────────────────────────────────────────

async fn diagnose_alias(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(alias): Path<String>,
) -> Result<Json<agent2ssh::remote::DaemonDiagnostic>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    diagnose_daemon(&alias)
        .await
        .map(Json)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

async fn diagnose_all(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<agent2ssh::remote::DaemonDiagnostic>>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let remotes =
        agent2ssh::remote::load_remotes().map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
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
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(alias): Path<String>,
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
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<TeamConfigExport>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    export_team_config()
        .map(Json)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn config_import(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(export): Json<TeamConfigExport>,
) -> Result<Json<ImportResult>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    import_team_config(&export)
        .map(Json)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

async fn config_import_preview(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(export): Json<TeamConfigExport>,
) -> Result<Json<ConfigDiffPreview>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    preview_team_config_import(&export)
        .map(Json)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

async fn proxy_exec(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(alias): Path<String>,
    Json(mut req): Json<ExecRequest>,
) -> Result<Json<ExecResult>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let auth = check_auth(&s, &headers)?;
    if req.source.is_none() {
        req.source = Some(source_from_env("daemon_proxy"));
    }
    let source = req.source.as_deref().unwrap_or("daemon_proxy").to_string();

    // If alias is "localhost", execute locally
    if alias == "localhost" {
        reject_if_gate_paused(&source, &req.host, &req.command)?;
        let targets = vec![(req.host.clone(), host_tags(&req.host))];
        reject_if_rate_limited(&s, &source, &targets, &req.command).await?;
        let tags = targets
            .first()
            .map(|(_, tags)| tags.clone())
            .unwrap_or_default();
        let (risk, approved) = authorize_command(
            &auth.scope,
            &source,
            &req.host,
            &tags,
            None,
            &req.command,
            req.force,
            req.reason.clone(),
            req.change_id.clone(),
        )
        .await?;
        if approved && risk == RiskLevel::High {
            req.force = true;
        }
        return exec_ssh_core(req)
            .await
            .map(Json)
            .map_err(|e| err(StatusCode::BAD_REQUEST, e));
    }

    // Look up remote daemon
    let (url, remote_token, scope) =
        get_daemon_with_scope(&alias).map_err(|e| err(StatusCode::NOT_FOUND, e))?;
    let tags = host_tags(&req.host);
    let targets = vec![(req.host.clone(), tags.clone())];
    reject_if_gate_paused(&source, &req.host, &req.command)?;
    reject_if_rate_limited(&s, &source, &targets, &req.command).await?;
    check_daemon_scope(&auth.scope, &req.host, &tags, &req.command)
        .map_err(|e| err(StatusCode::FORBIDDEN, e))?;
    check_daemon_scope(&scope, &req.host, &tags, &req.command)
        .map_err(|e| err(StatusCode::FORBIDDEN, e))?;
    let token = remote_token.ok_or_else(|| {
        err(
            StatusCode::UNAUTHORIZED,
            format!("no token configured for daemon '{}'", alias),
        )
    })?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            req.timeout_secs.unwrap_or(60) + 10,
        ))
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

    let result: ExecResult = resp.json().await.map_err(|e| {
        err(
            StatusCode::BAD_GATEWAY,
            format!("invalid response from remote: {e}"),
        )
    })?;
    Ok(Json(result))
}

// ── Daemons Unified View (F5-4) ─────────────────────────────────────────────

async fn daemons_view(
    State(s): State<AppState>,
    headers: HeaderMap,
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
    State(s): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<TrendQuery>,
) -> Result<Json<MetricsTrend>, (StatusCode, Json<ErrorBody>)> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    check_auth(&s, &headers)?;
    let period = match q.period.as_deref().unwrap_or("24h") {
        "24h" | "last24h" => TrendPeriod::Last24h,
        "7d" | "last7d" => TrendPeriod::Last7d,
        "30d" | "last30d" => TrendPeriod::Last30d,
        "all" => TrendPeriod::All,
        other => {
            return Err(err(
                StatusCode::BAD_REQUEST,
                format!("unknown period '{}'. Use: 24h, 7d, 30d, or all", other),
            ))
        }
    };
    compute_metrics_trend(period)
        .map(Json)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

// ── Event Stream SSE (F6-4) ─────────────────────────────────────────────────

async fn events_stream(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<
    Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>>,
    (StatusCode, Json<ErrorBody>),
> {
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
            Err(_) => None,
        }
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

// ── SSH config sync (F2-4) ───────────────────────────────────────────────────

async fn ssh_sync_diff(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let diff = agent2ssh::compare_ssh_configs(None).map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("ssh sync diff failed: {e}"),
        )
    })?;
    Ok(Json(serde_json::to_value(diff).unwrap_or_default()))
}

async fn ssh_sync_export_handler(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let (path, count) = agent2ssh::export_to_ssh_config(None, None).map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("ssh sync export failed: {e}"),
        )
    })?;
    Ok(Json(
        serde_json::json!({ "path": path, "hosts_exported": count }),
    ))
}

// ── WebSocket streaming exec ─────────────────────────────────────────────────

async fn exec_stream(
    State(s): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    // Fix-2: Authenticate before WebSocket upgrade
    let auth = match check_auth(&s, &headers) {
        Ok(auth) => auth,
        Err(e) => return e.into_response(),
    };
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let app_state = s.clone();
    let auth_scope = auth.scope.clone();
    ws.on_upgrade(|socket| async move {
        use axum::extract::ws::Message;
        use std::sync::Arc;
        use tokio::io::AsyncReadExt;

        let socket = Arc::new(tokio::sync::Mutex::new(socket));

        // Wait for ExecRequest message
        let req_msg = {
            let mut s = socket.lock().await;
            match s.recv().await {
                Some(Ok(Message::Text(text))) => text,
                _ => return,
            }
        };
        let mut req: ExecRequest = match serde_json::from_str(&req_msg) {
            Ok(r) => r,
            Err(e) => {
                let mut s = socket.lock().await;
                let _ = s
                    .send(Message::Text(
                        serde_json::json!({"type":"error","error":e.to_string()}).to_string(),
                    ))
                    .await;
                return;
            }
        };
        let source = req
            .source
            .clone()
            .unwrap_or_else(|| source_from_env("daemon_ws"));
        req.source = Some(source.clone());
        if reject_if_gate_paused(&source, &req.host, &req.command).is_err() {
            let mut s = socket.lock().await;
            let _ = s
                .send(Message::Text(
                    serde_json::json!({"type":"error","error":"execution gate paused"}).to_string(),
                ))
                .await;
            return;
        }
        let targets = vec![(req.host.clone(), host_tags(&req.host))];
        if let Err((_, Json(body))) =
            reject_if_rate_limited(&app_state, &source, &targets, &req.command).await
        {
            let mut s = socket.lock().await;
            let _ = s
                .send(Message::Text(
                    serde_json::json!({"type":"error","error":body.error}).to_string(),
                ))
                .await;
            return;
        }

        let tags = targets
            .first()
            .map(|(_, tags)| tags.clone())
            .unwrap_or_default();
        let risk = match authorize_command(
            &auth_scope,
            &source,
            &req.host,
            &tags,
            None,
            &req.command,
            req.force,
            req.reason.clone(),
            req.change_id.clone(),
        )
        .await
        {
            Ok((risk, approved)) => {
                if approved && risk == RiskLevel::High {
                    req.force = true;
                }
                risk
            }
            Err((_, Json(body))) => {
                let mut s = socket.lock().await;
                let _ = s
                    .send(Message::Text(
                        serde_json::json!({"type":"error","error":body.error}).to_string(),
                    ))
                    .await;
                return;
            }
        };
        if risk == RiskLevel::High && !req.force {
            let mut s = socket.lock().await;
            let _ = s
                .send(Message::Text(
                    serde_json::json!({"type":"error","error":"force required"}).to_string(),
                ))
                .await;
            return;
        }

        let host = match load_config()
            .ok()
            .and_then(|c| c.hosts.into_iter().find(|h| h.name == req.host))
        {
            Some(h) => h,
            None => {
                let mut s = socket.lock().await;
                let _ = s
                    .send(Message::Text(
                        serde_json::json!({"type":"error","error":"unknown host"}).to_string(),
                    ))
                    .await;
                return;
            }
        };

        let started = std::time::Instant::now();
        let timeout_secs = req.timeout_secs.unwrap_or(60);
        publish_event(
            EventType::ExecStarted,
            serde_json::json!({
                "source": source,
                "host": req.host,
                "command": req.command,
                "reason": req.reason.clone(),
                "change_id": req.change_id.clone(),
                "risk_level": format!("{}", risk),
            }),
        );

        let mut cmd = build_ssh_exec_command(&host, &req.command).await;
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let mut s = socket.lock().await;
                let _ = s
                    .send(Message::Text(
                        serde_json::json!({"type":"error","error":e.to_string()}).to_string(),
                    ))
                    .await;
                return;
            }
        };

        // Fix-3: Concurrently stream both stdout and stderr
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let sock_out = socket.clone();
        let out_host = req.host.clone();
        let out_command = req.command.clone();
        let out_source = source.clone();
        let stdout_task = tokio::spawn(async move {
            if let Some(stdout) = stdout {
                let mut reader = tokio::io::BufReader::new(stdout);
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let data = String::from_utf8_lossy(&buf[..n]).to_string();
                            publish_event(
                                EventType::ExecOutput,
                                serde_json::json!({
                                    "source": out_source,
                                    "host": out_host,
                                    "command": out_command,
                                    "stream": "stdout",
                                    "output_preview": preview_text(&data, 4096),
                                    "output_bytes": data.len(),
                                }),
                            );
                            let mut s = sock_out.lock().await;
                            if s.send(Message::Text(
                                serde_json::json!({"type":"stdout","data":data}).to_string(),
                            ))
                            .await
                            .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            }
        });

        let sock_err = socket.clone();
        let err_host = req.host.clone();
        let err_command = req.command.clone();
        let err_source = source.clone();
        let stderr_task = tokio::spawn(async move {
            if let Some(stderr) = stderr {
                let mut reader = tokio::io::BufReader::new(stderr);
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let data = String::from_utf8_lossy(&buf[..n]).to_string();
                            publish_event(
                                EventType::ExecOutput,
                                serde_json::json!({
                                    "source": err_source,
                                    "host": err_host,
                                    "command": err_command,
                                    "stream": "stderr",
                                    "output_preview": preview_text(&data, 4096),
                                    "output_bytes": data.len(),
                                }),
                            );
                            let mut s = sock_err.lock().await;
                            if s.send(Message::Text(
                                serde_json::json!({"type":"stderr","data":data}).to_string(),
                            ))
                            .await
                            .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            }
        });

        // Wait for child process with timeout
        let status =
            tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), child.wait()).await;

        let _ = tokio::join!(stdout_task, stderr_task);

        let code = match status {
            Ok(Ok(s)) => s.code(),
            Ok(Err(_)) => None,
            Err(_) => {
                let _ = child.kill().await;
                None
            }
        };
        let duration_ms = started.elapsed().as_millis();
        let completed_host = req.host.clone();
        let completed_command = req.command.clone();
        let audit_result = ExecResult {
            host: completed_host.clone(),
            command: completed_command.clone(),
            exit_code: code,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms,
            risk_level: risk,
            truncated: false,
        };
        let _ = append_audit(
            &audit_result,
            risk,
            req.reason.as_deref(),
            req.change_id.as_deref(),
            Some(&source),
        );
        publish_event(
            EventType::ExecCompleted,
            serde_json::json!({
                "source": source,
                "host": completed_host,
                "command": completed_command,
                "exit_code": code,
                "risk_level": format!("{}", risk),
                "duration_ms": duration_ms,
            }),
        );

        let mut s = socket.lock().await;
        let _ = s
            .send(Message::Text(
                serde_json::json!({"type":"exit","code":code,"duration_ms":duration_ms})
                    .to_string(),
            ))
            .await;
    })
}

fn preview_text(value: &str, max_chars: usize) -> String {
    let redacted = redact_sensitive_text(value);
    let mut preview: String = redacted.chars().take(max_chars).collect();
    if redacted.chars().count() > max_chars {
        preview.push_str("\n...[truncated]");
    }
    preview
}

/// Check whether a binary exists on PATH (used by health + doctor).
pub fn which_binary(name: &str) -> Option<String> {
    let output = std::process::Command::new("which")
        .arg(name)
        .output()
        .ok()?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(path);
        }
    }
    None
}

// ── Health Snapshot ─────────────────────────────────────────────────────────

async fn get_health_snapshot(
    State(s): State<AppState>,
    headers: HeaderMap,
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
    State(s): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<HealthSnapshotBody>>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use agent2ssh::limits::ExecutionLimitConfig;
    use std::sync::{Mutex, OnceLock};

    fn with_temp_config<T>(f: impl FnOnce() -> T) -> T {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let temp =
            std::env::temp_dir().join(format!("agent2ssh-daemon-gate-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &temp);
        let out = f();
        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&temp);
        out
    }

    fn test_state(config: ExecutionLimitConfig) -> AppState {
        AppState {
            token: "test-token".into(),
            limiter: Arc::new(tokio::sync::Mutex::new(ExecutionLimiter::new(config))),
        }
    }

    #[test]
    fn token_match_requires_exact_non_empty_value() {
        assert!(token_matches("secret-token", "secret-token"));
        assert!(!token_matches("secret-token", "secret-tokem"));
        assert!(!token_matches("secret", "secret-token"));
        assert!(!token_matches("secret-token-extra", "secret-token"));
        assert!(!token_matches("", ""));
        assert!(!token_matches("anything", "   "));
    }

    #[test]
    fn daemon_token_loader_preserves_existing_non_empty_token() {
        let temp =
            std::env::temp_dir().join(format!("agent2ssh-daemon-token-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        let token_path = temp.join("daemon.token");
        std::fs::write(&token_path, "existing-token\n").unwrap();

        let token = load_or_create_daemon_token(&temp).unwrap();
        let stored = std::fs::read_to_string(&token_path).unwrap();
        let _ = std::fs::remove_dir_all(&temp);

        assert_eq!(token, "existing-token");
        assert_eq!(stored, "existing-token\n");
    }

    #[test]
    fn daemon_token_loader_replaces_empty_token_file() {
        let temp =
            std::env::temp_dir().join(format!("agent2ssh-daemon-token-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        let token_path = temp.join("daemon.token");
        std::fs::write(&token_path, " \n").unwrap();

        let token = load_or_create_daemon_token(&temp).unwrap();
        let stored = std::fs::read_to_string(&token_path).unwrap();
        let _ = std::fs::remove_dir_all(&temp);

        assert!(!token.trim().is_empty());
        assert_eq!(stored.trim(), token);
        assert!(token_matches(&token, stored.trim()));
    }

    #[test]
    fn paused_gate_rejects_non_desktop_source_and_writes_audit() {
        with_temp_config(|| {
            save_execution_gate(
                ExecutionGateMode::Paused,
                Some("desktop".into()),
                Some("maintenance".into()),
            )
            .unwrap();

            let rejected = reject_if_gate_paused("mcp", "test-host", "uptime");
            assert!(rejected.is_err());
            let (status, _) = rejected.unwrap_err();
            assert_eq!(status, locked_status());

            let audit = list_audit_core(AuditFilter {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
            let entry = audit.first().expect("gate rejection should write audit");
            assert_eq!(entry.host, "test-host");
            assert_eq!(entry.command, "uptime");
            assert_eq!(entry.risk_level, RiskLevel::Blocked);
            assert_eq!(entry.source.as_deref(), Some("mcp"));
            assert_eq!(entry.reason.as_deref(), Some("maintenance"));
        });
    }

    #[test]
    fn paused_gate_allows_desktop_source() {
        with_temp_config(|| {
            save_execution_gate(ExecutionGateMode::Paused, Some("desktop".into()), None).unwrap();
            assert!(reject_if_gate_paused("desktop", "test-host", "uptime").is_ok());
        });
    }

    #[test]
    fn rate_limit_rejects_with_429_and_writes_audit() {
        with_temp_config(|| {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let state = test_state(ExecutionLimitConfig {
                    default_source_per_minute: 1,
                    default_host_per_minute: 0,
                    default_tag_per_minute: 0,
                    ..Default::default()
                });
                let targets = vec![("test-host".to_string(), vec![])];
                assert!(reject_if_rate_limited(&state, "mcp", &targets, "uptime")
                    .await
                    .is_ok());
                let err = reject_if_rate_limited(&state, "mcp", &targets, "uptime")
                    .await
                    .unwrap_err();
                assert_eq!(err.0, too_many_requests_status());

                let audit = list_audit_core(AuditFilter {
                    limit: 10,
                    ..Default::default()
                })
                .unwrap();
                let entry = audit.first().expect("limit rejection should write audit");
                assert_eq!(entry.host, "test-host");
                assert_eq!(entry.command, "uptime");
                assert_eq!(entry.risk_level, RiskLevel::Blocked);
                assert_eq!(entry.source.as_deref(), Some("mcp"));
                assert!(entry
                    .reason
                    .as_deref()
                    .unwrap_or_default()
                    .contains("source:mcp rate"));
            })
        });
    }

    #[test]
    fn session_limit_rejects_with_429() {
        with_temp_config(|| {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let state = test_state(ExecutionLimitConfig {
                    default_source_max_sessions: 0,
                    default_host_max_sessions: 1,
                    default_tag_max_sessions: 0,
                    ..Default::default()
                });
                state.limiter.lock().await.register_session(
                    Uuid::new_v4(),
                    "mcp",
                    "test-host",
                    &[],
                );
                let err = reject_if_session_limited(&state, "cli", "test-host", &[])
                    .await
                    .unwrap_err();
                assert_eq!(err.0, too_many_requests_status());
            })
        });
    }

    #[test]
    fn scoped_token_auth_restricts_authorized_commands() {
        with_temp_config(|| {
            std::fs::write(
                config_dir().unwrap().join("daemon_tokens.toml"),
                r#"
[[tokens]]
name = "readonly"
token = "scoped-token"

[tokens.scope]
allowed_hosts = ["prod"]
allowed_commands = ["uptime", "ls *"]
denied_commands = ["rm *"]
"#,
            )
            .unwrap();

            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let state = test_state(ExecutionLimitConfig::default());
                let mut headers = HeaderMap::new();
                headers.insert("authorization", "Bearer scoped-token".parse().unwrap());
                let auth = check_auth(&state, &headers).unwrap();
                assert!(auth.scope.is_some());

                authorize_command(
                    &auth.scope,
                    "mcp",
                    "prod",
                    &[],
                    None,
                    "uptime",
                    false,
                    None,
                    None,
                )
                .await
                .unwrap();

                let denied = authorize_command(
                    &auth.scope,
                    "mcp",
                    "prod",
                    &[],
                    None,
                    "cat /etc/passwd",
                    false,
                    None,
                    None,
                )
                .await
                .unwrap_err();
                assert_eq!(denied.0, StatusCode::FORBIDDEN);

                let denied_host = authorize_command(
                    &auth.scope,
                    "mcp",
                    "dev",
                    &[],
                    None,
                    "uptime",
                    false,
                    None,
                    None,
                )
                .await
                .unwrap_err();
                assert_eq!(denied_host.0, StatusCode::FORBIDDEN);
            })
        });
    }
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
            tracing_subscriber::fmt().with_env_filter(env_filter).init();
        }
    }

    // Record start time for uptime calculation
    START_TIME.get_or_init(Instant::now);

    let addr = "127.0.0.1:7722";
    let config_dir = config_dir()?;
    std::fs::create_dir_all(&config_dir)?;

    // Token
    let token = load_or_create_daemon_token(&config_dir)?;

    // PID
    let pid_path = config_dir.join("daemon.pid");
    std::fs::write(&pid_path, std::process::id().to_string())?;

    let limits = load_execution_limits()?;
    let state = AppState {
        token: token.clone(),
        limiter: Arc::new(Mutex::new(ExecutionLimiter::new(limits))),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route(
            "/",
            get(|| async { axum::response::Redirect::to("/console") }),
        )
        .route("/console", get(serve_console))
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/metrics/trend", get(metrics_trend))
        .route("/gate", get(gate_status))
        .route("/gate/pause", post(gate_pause))
        .route("/gate/resume", post(gate_resume))
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
        .route(
            "/webhook/config",
            get(get_webhook_config).put(set_webhook_config),
        )
        .route(
            "/health-snapshot",
            get(get_health_snapshot).post(post_health_snapshot),
        )
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
