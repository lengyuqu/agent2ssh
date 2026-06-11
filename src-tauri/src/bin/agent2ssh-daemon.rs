use agent2ssh::approval::{approval_list, approval_request, approval_respond, approval_wait, ApprovalStatus};
use agent2ssh::approval::ApprovalRequest as ApprovalRequestType;
use agent2ssh::core::*;
use agent2ssh::forward::*;
use agent2ssh::risk_config::classify_with_user_rules;
use agent2ssh::session::*;
use agent2ssh::store::*;
use agent2ssh::types::*;

use axum::{
    extract::{Path, Query, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

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
#[derive(Deserialize)] struct ExecMultiBody { hosts: Vec<String>, command: String, #[serde(default)] force: bool, timeout_secs: Option<u64> }
#[derive(Deserialize)] struct SftpDirBody { host: String, path: String }
#[derive(Deserialize)] struct SessionOpenBody { host: String }
#[derive(Deserialize)] struct SessionWriteBody { input: String }
#[derive(Deserialize)] struct ReadQuery { timeout_ms: Option<u64> }
#[derive(Deserialize)] struct AuditQuery { host: Option<String>, risk_level: Option<RiskLevel>, exit_code: Option<i32>, since: Option<String>, until: Option<String>, limit: Option<usize> }
#[derive(Deserialize)] struct RiskCheckBody { command: String, #[allow(dead_code)] host: Option<String> }
#[derive(Serialize)] struct RiskCheckResult { risk_level: RiskLevel, matched_rule: Option<String> }
#[derive(Serialize)] struct OkBody { ok: bool }
#[derive(Serialize)] struct IdBody { id: String }
#[derive(Serialize)] struct SessionListItem { id: String, host: String }
#[derive(Serialize)] struct OutputBody { output: String }

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"ok": true}))
}

async fn list_hosts(
    State(s): State<AppState>, headers: HeaderMap,
) -> Result<Json<Vec<HostProfile>>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    list_hosts_core().map(Json).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn add_host(
    State(s): State<AppState>, headers: HeaderMap, Json(host): Json<HostProfile>,
) -> Result<Json<HostProfile>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    add_host_core(host).map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

async fn remove_host(
    State(s): State<AppState>, headers: HeaderMap, Path(name): Path<String>,
) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    remove_host_core(&name).map(|_| Json(OkBody { ok: true })).map_err(|e| err(StatusCode::NOT_FOUND, e))
}

async fn import_config(
    State(s): State<AppState>, headers: HeaderMap,
) -> Result<Json<Vec<HostProfile>>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    import_ssh_config_core(None).map(Json).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn ping(
    State(s): State<AppState>, headers: HeaderMap, Json(body): Json<PingBody>,
) -> Result<Json<Vec<PingResult>>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    Ok(Json(ping_hosts_core(body.hosts, body.timeout_secs).await))
}

async fn exec(
    State(s): State<AppState>, headers: HeaderMap, Json(req): Json<ExecRequest>,
) -> Result<Json<ExecResult>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    // Check user-defined rules first
    if let Some(user_risk) = classify_with_user_rules(&req.command).await {
        if user_risk == RiskLevel::Blocked {
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
        let approval_id = approval_request(&req.host, &req.command, effective_risk).await;
        let status = approval_wait(approval_id).await;
        match status {
            ApprovalStatus::Approved => {
                // Execute with force
                let mut approved_req = req;
                approved_req.force = true;
                exec_ssh_core(approved_req).await.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
            }
            ApprovalStatus::Rejected => {
                Err(err(StatusCode::FORBIDDEN, "command rejected by approver"))
            }
            ApprovalStatus::TimedOut => {
                Err(err(StatusCode::REQUEST_TIMEOUT, "approval request timed out"))
            }
            _ => {
                Err(err(StatusCode::INTERNAL_SERVER_ERROR, "unexpected approval status"))
            }
        }
    } else {
        exec_ssh_core(req).await.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
    }
}

async fn exec_multi(
    State(s): State<AppState>, headers: HeaderMap, Json(body): Json<ExecMultiBody>,
) -> Result<Json<Vec<ExecMultiResult>>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    Ok(Json(exec_multi_core(body.hosts, body.command, body.force, body.timeout_secs).await))
}

async fn audit(
    State(s): State<AppState>, headers: HeaderMap, Query(q): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEntry>>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    let filter = AuditFilter { host: q.host, risk_level: q.risk_level, exit_code: q.exit_code, since: q.since, until: q.until, limit: q.limit.unwrap_or(20) };
    list_audit_core(filter).map(Json).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

// ── SFTP ─────────────────────────────────────────────────────────────────────

async fn sftp_upload(State(s): State<AppState>, headers: HeaderMap, Json(req): Json<SftpUploadRequest>) -> Result<Json<SftpResult>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?; sftp_upload_core(req).await.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn sftp_download(State(s): State<AppState>, headers: HeaderMap, Json(req): Json<SftpDownloadRequest>) -> Result<Json<SftpResult>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?; sftp_download_core(req).await.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn sftp_ls(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<SftpDirBody>) -> Result<Json<ExecResult>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?; sftp_ls_core(&body.host, &body.path, None).await.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn sftp_stat(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<SftpDirBody>) -> Result<Json<ExecResult>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?; sftp_stat_core(&body.host, &body.path, None).await.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn sftp_mkdir(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<SftpDirBody>) -> Result<Json<ExecResult>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?; sftp_mkdir_core(&body.host, &body.path, None).await.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

// ── Sessions ─────────────────────────────────────────────────────────────────

async fn session_open(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<SessionOpenBody>) -> Result<Json<IdBody>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    session_open_core(&body.host).await.map(|id| Json(IdBody { id: id.to_string() })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn session_write(State(s): State<AppState>, headers: HeaderMap, Path(id): Path<String>, Json(body): Json<SessionWriteBody>) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    session_write_core(uuid, &body.input).await.map(|_| Json(OkBody { ok: true })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn session_read(State(s): State<AppState>, headers: HeaderMap, Path(id): Path<String>, Query(q): Query<ReadQuery>) -> Result<Json<OutputBody>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    session_read_core(uuid, q.timeout_ms.unwrap_or(2000)).await.map(|output| Json(OutputBody { output })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn session_close(State(s): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    session_close_core(uuid).await.map(|_| Json(OkBody { ok: true })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn session_list(State(s): State<AppState>, headers: HeaderMap) -> Result<Json<Vec<SessionListItem>>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    Ok(Json(session_list_core().await.into_iter().map(|(id, host)| SessionListItem { id: id.to_string(), host }).collect()))
}

// ── Forwards ─────────────────────────────────────────────────────────────────

async fn forward_add(State(s): State<AppState>, headers: HeaderMap, Json(req): Json<ForwardRule>) -> Result<Json<ForwardRule>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    forward_add_core(&req.host, req.direction, req.bind_port, &req.target_host, req.target_port).await.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn forward_list(State(s): State<AppState>, headers: HeaderMap) -> Result<Json<Vec<ForwardRule>>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    Ok(Json(forward_list_core().await))
}
async fn forward_remove(State(s): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    forward_remove_core(uuid).await.map(|_| Json(OkBody { ok: true })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

// ── Approvals ────────────────────────────────────────────────────────────────

async fn approvals_list(State(s): State<AppState>, headers: HeaderMap) -> Result<Json<Vec<ApprovalRequestType>>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    Ok(Json(approval_list().await))
}
async fn approval_approve(State(s): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    approval_respond(uuid, true).await.map(|_| Json(OkBody { ok: true })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}
async fn approval_reject(State(s): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> Result<Json<OkBody>, (StatusCode, Json<ErrorBody>)> {
    check_auth(&s, &headers)?;
    let uuid = Uuid::parse_str(&id).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    approval_respond(uuid, false).await.map(|_| Json(OkBody { ok: true })).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

// ── Risk check ───────────────────────────────────────────────────────────────

async fn risk_check(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<RiskCheckBody>) -> Result<Json<RiskCheckResult>, (StatusCode, Json<ErrorBody>)> {
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

// ── WebSocket streaming exec ─────────────────────────────────────────────────

async fn exec_stream(State(_s): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(|mut socket| async move {
        use axum::extract::ws::Message;
        use tokio::io::AsyncReadExt;

        // Wait for ExecRequest message
        let req_msg = match socket.recv().await {
            Some(Ok(Message::Text(text))) => text,
            _ => return,
        };
        let req: ExecRequest = match serde_json::from_str(&req_msg) {
            Ok(r) => r,
            Err(e) => { let _ = socket.send(Message::Text(serde_json::json!({"type":"error","error":e.to_string()}).to_string())).await; return; }
        };

        let risk = classify_risk(&req.command);
        if risk == RiskLevel::Blocked || (risk == RiskLevel::High && !req.force) {
            let _ = socket.send(Message::Text(serde_json::json!({"type":"error","error":"blocked or force required"}).to_string())).await;
            return;
        }

        let host = match load_config().ok().and_then(|c| c.hosts.into_iter().find(|h| h.name == req.host)) {
            Some(h) => h,
            None => { let _ = socket.send(Message::Text(serde_json::json!({"type":"error","error":"unknown host"}).to_string())).await; return; }
        };

        let started = std::time::Instant::now();
        let timeout_secs = req.timeout_secs.unwrap_or(60);

        let mut cmd = tokio::process::Command::new("ssh");
        cmd.arg("-o").arg("BatchMode=yes").arg("-o").arg("StrictHostKeyChecking=accept-new")
           .arg("-p").arg(host.port.unwrap_or(22).to_string());
        if let Some(kp) = &host.key_path { if !kp.trim().is_empty() { cmd.arg("-i").arg(expand_tilde(kp)); } }
        let target = match &host.user { Some(u) if !u.trim().is_empty() => format!("{}@{}", u, host.host), _ => host.host.clone() };
        cmd.arg(&target).arg(&req.command)
           .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).stdin(std::process::Stdio::null());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => { let _ = socket.send(Message::Text(serde_json::json!({"type":"error","error":e.to_string()}).to_string())).await; return; }
        };

        if let Some(stdout) = child.stdout.take() {
            let mut reader = tokio::io::BufReader::new(stdout);
            let mut buf = [0u8; 4096];
            loop {
                match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), reader.read(&mut buf)).await {
                    Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
                    Ok(Ok(n)) => {
                        let data = String::from_utf8_lossy(&buf[..n]).to_string();
                        if socket.send(Message::Text(serde_json::json!({"type":"stdout","data":data}).to_string())).await.is_err() { break; }
                    }
                }
            }
        }

        let status = child.wait().await;
        let code = status.ok().and_then(|s| s.code());
        let _ = socket.send(Message::Text(serde_json::json!({"type":"exit","code":code,"duration_ms":started.elapsed().as_millis()}).to_string())).await;
    })
}

fn expand_tilde(path: &str) -> String {
    if path == "~" { return dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_else(|| path.to_string()); }
    if let Some(rest) = path.strip_prefix("~/") { if let Some(home) = dirs::home_dir() { return home.join(rest).display().to_string(); } }
    path.to_string()
}

// ── Web Console ──────────────────────────────────────────────────────────────

async fn serve_console() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("../../web/console.html"))
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr = "127.0.0.1:7722";
    let config_dir = config_dir()?;
    std::fs::create_dir_all(&config_dir)?;

    // Token
    let token_path = config_dir.join("daemon.token");
    let token = if token_path.exists() {
        std::fs::read_to_string(&token_path)?.trim().to_string()
    } else {
        let t = Uuid::new_v4().to_string();
        std::fs::write(&token_path, &t)?;
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
        .route("/hosts", get(list_hosts).post(add_host))
        .route("/hosts/import", post(import_config))
        .route("/hosts/:name", delete(remove_host))
        .route("/ping", post(ping))
        .route("/exec", post(exec))
        .route("/exec-multi", post(exec_multi))
        .route("/exec/stream", get(exec_stream))
        .route("/audit", get(audit))
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
        .route("/risk/check", post(risk_check))
        .layer(cors)
        .with_state(state);

    println!("Agent2SSH daemon listening on {addr}");
    println!("Web console: http://{addr}/console");
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    let _ = std::fs::remove_file(&pid_path);
    Ok(())
}
