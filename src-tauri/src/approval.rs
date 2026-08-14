use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::app_state::app_state;
use crate::types::RiskLevel;

/// Default TTL for approval requests in seconds (5 minutes).
/// Can be overridden per-request by setting `ttl_secs` on the `ApprovalRequest`
/// after creation, or by using `approval_request_with_ttl`.
pub const DEFAULT_APPROVAL_TTL_SECS: u64 = 300;

// ── Approval Context Types ──────────────────────────────────────────────────

/// Rich context attached to an approval request, providing the approver with
/// additional information about the target host, execution history, and risk.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApprovalContext {
    /// Target host address (IP or hostname)
    #[serde(default)]
    pub host_address: Option<String>,
    /// Host environment label (prod, staging, etc.)
    #[serde(default)]
    pub host_env: Option<String>,
    /// Host role label
    #[serde(default)]
    pub host_role: Option<String>,
    /// Request source (cli, mcp, daemon, web)
    #[serde(default)]
    pub source: Option<String>,
    /// Recent execution history on this host (last N commands)
    #[serde(default)]
    pub recent_commands: Vec<ApprovalHistoryEntry>,
    /// Risk breakdown
    #[serde(default)]
    pub risk_details: Option<RiskDetails>,
    /// Optional reason provided by the requester
    #[serde(default)]
    pub reason: Option<String>,
    /// Optional change/ticket ID
    #[serde(default)]
    pub change_id: Option<String>,
}

/// A single historical execution entry used in approval context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalHistoryEntry {
    pub command: String,
    pub exit_code: Option<i32>,
    pub risk_level: RiskLevel,
    pub ts: String, // ISO-8601 timestamp string
}

/// Detailed risk breakdown showing how the final risk level was determined.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskDetails {
    pub built_in_risk: RiskLevel,
    pub user_risk_override: Option<RiskLevel>,
    pub final_risk: RiskLevel,
    pub matched_policy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: Uuid,
    pub host: String,
    pub command: String,
    pub risk_level: RiskLevel,
    pub requested_at: DateTime<Utc>,
    pub ttl_secs: u64,
    pub status: ApprovalStatus,
    /// Optional rich context for the approval request.
    #[serde(default)]
    pub context: Option<ApprovalContext>,
    /// T2-14: Timestamp when the approval was revoked (if applicable).
    #[serde(default)]
    pub revoked_at: Option<DateTime<Utc>>,
    /// T2-14: Command snapshot taken at approval time. This captures the
    /// exact command text that was approved, so if the original `command`
    /// field is later modified (e.g. by a concurrent request), we can still
    /// verify what was actually approved.
    #[serde(default)]
    pub approved_command_snapshot: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    TimedOut,
    /// T2-14: Approval was revoked after being approved.
    Revoked,
}

pub struct ApprovalStore {
    pub requests: HashMap<Uuid, ApprovalRequest>,
}

fn store() -> &'static Mutex<ApprovalStore> {
    &app_state().approvals
}

/// Whether approval state is persisted to `approvals.json` and shared across
/// CLI/MCP/daemon/desktop processes. Defaults to **enabled** — the previous
/// opt-in (`AGENT2SSH_APPROVAL_PERSIST=1`) meant approvals lived in per-process
/// memory by default, so a high-risk command queued in the daemon was invisible
/// to the desktop App until it refetched. Set `AGENT2SSH_APPROVAL_PERSIST=0`
/// to restore the old in-memory-only behavior (testing only).
fn approval_persistence_enabled() -> bool {
    // Tests must never touch the real ~/.agent2ssh/approvals.json. The
    // thread_local config_dir override (store::THREAD_CONFIG_DIR) doesn't help
    // here because approval persistence writes to that dir too — and parallel
    // tests would still race on the same file. Force-disable in test builds.
    #[cfg(test)]
    {
        if std::env::var("AGENT2SSH_APPROVAL_PERSIST_TEST").as_deref() != Ok("1") {
            return false;
        }
    }
    std::env::var("AGENT2SSH_APPROVAL_PERSIST").as_deref() != Ok("0")
}

/// Current JSONL store path (append-only, one `ApprovalRequest` per line).
fn approval_store_path() -> Result<PathBuf> {
    Ok(crate::store::config_dir()?.join("approvals.jsonl"))
}

/// Legacy JSON-array store path (pre-JSONL migration). When present at startup
/// we read it, migrate entries into `approvals.jsonl`, and rename it to
/// `approvals.json.migrated` so the migration runs at most once per install.
fn legacy_approval_store_path() -> Result<PathBuf> {
    Ok(crate::store::config_dir()?.join("approvals.json"))
}

/// Cross-process file lock for the approval store. Held exclusively by writers
/// and as a shared lock by readers. Prevents the read-then-write race where
/// two processes both load the same store, both add an entry, and the second
/// write clobbers the first.
fn approval_file_lock() -> Result<crate::store::FileLockGuard> {
    crate::store::lock_config_file(".approvals.lock")
}

/// Maximum age (in days) of approval records kept on disk. Entries older than
/// this are removed during `cleanup_expired_approvals` (called at startup and
/// periodically by the daemon). Tune via `AGENT2SSH_APPROVAL_RETENTION_DAYS`.
const APPROVAL_RETENTION_DAYS_DEFAULT: i64 = 30;

fn approval_retention_days() -> i64 {
    std::env::var("AGENT2SSH_APPROVAL_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(APPROVAL_RETENTION_DAYS_DEFAULT)
}

/// Load the persisted approval store from disk. Reads the JSONL file line by
/// line; later lines for the same ID override earlier ones (last-write-wins),
/// so an append of a status update naturally supersedes the original request.
///
/// Also runs a one-time migration from the legacy `approvals.json` array
/// format if it exists, and cleans up entries older than the retention window.
pub fn load_persisted_approval_store() -> Result<ApprovalStore> {
    if !approval_persistence_enabled() {
        return Ok(ApprovalStore {
            requests: HashMap::new(),
        });
    }
    let _guard = approval_file_lock()?;
    crate::store::ensure_config_dir()?;

    // One-time migration from legacy JSON-array format.
    migrate_legacy_approvals_unlocked()?;

    let path = approval_store_path()?;
    if !path.exists() {
        return Ok(ApprovalStore {
            requests: HashMap::new(),
        });
    }

    // Read JSONL: each line is one ApprovalRequest. Later lines override
    // earlier lines with the same ID (append semantics — see append_approval).
    let mut requests: HashMap<Uuid, ApprovalRequest> = HashMap::new();
    let raw = std::fs::read_to_string(&path)?;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<ApprovalRequest>(line) {
            Ok(req) => {
                requests.insert(req.id, req);
            }
            Err(e) => {
                // Skip malformed lines rather than failing the whole load —
                // a corrupted line shouldn't prevent the daemon from starting.
                let _ = crate::diagnostics::append_diagnostic_log_no_sink(
                    "warn",
                    "approval",
                    "skipping malformed approval line",
                    Some(serde_json::json!({ "error": e.to_string() })),
                );
            }
        }
    }

    // Drop entries beyond the retention window so the file doesn't grow
    // unboundedly across months of daemon uptime. We do this in-memory after
    // loading; the next `append_approval` of any entry will trigger a
    // compaction that rewrites the file without the dropped entries.
    let retention_days = approval_retention_days();
    let cutoff = Utc::now() - chrono::Duration::days(retention_days);
    let before = requests.len();
    requests.retain(|_, req| req.requested_at > cutoff);
    let dropped = before - requests.len();
    if dropped > 0 {
        let _ = crate::diagnostics::append_diagnostic_log_no_sink(
            "info",
            "approval",
            "dropped expired approval entries during load",
            Some(serde_json::json!({ "dropped": dropped, "retention_days": retention_days })),
        );
        // Rewrite the file without the expired entries. Best-effort — if it
        // fails, we'll just reload them next time, which is fine.
        let _ = compact_approvals_unlocked(&requests);
    }

    Ok(ApprovalStore { requests })
}

/// One-time migration from the legacy `approvals.json` (JSON array) format to
/// the current `approvals.jsonl` (JSONL) format. After a successful migration,
/// the legacy file is renamed to `approvals.json.migrated` so this is idempotent.
fn migrate_legacy_approvals_unlocked() -> Result<()> {
    let legacy = legacy_approval_store_path()?;
    if !legacy.exists() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(&legacy)?;
    let entries: Vec<ApprovalRequest> = serde_json::from_str(&raw)?;

    let jsonl_path = approval_store_path()?;
    let mut existing: HashMap<Uuid, ApprovalRequest> = HashMap::new();
    if jsonl_path.exists() {
        if let Ok(existing_raw) = std::fs::read_to_string(&jsonl_path) {
            for line in existing_raw.lines() {
                if let Ok(req) = serde_json::from_str::<ApprovalRequest>(line.trim()) {
                    existing.insert(req.id, req);
                }
            }
        }
    }
    // Legacy entries win — they predate the JSONL file.
    for req in entries {
        existing.insert(req.id, req);
    }
    compact_approvals_unlocked(&existing)?;

    // Mark the legacy file as migrated so we don't process it again.
    let migrated = legacy.with_extension("json.migrated");
    let _ = std::fs::rename(&legacy, &migrated);

    let _ = crate::diagnostics::append_diagnostic_log_no_sink(
        "info",
        "approval",
        "migrated legacy approvals.json to approvals.jsonl",
        Some(serde_json::json!({ "entries": existing.len() })),
    );
    Ok(())
}

/// Rewrite the JSONL file from scratch from the given entries. Used by
/// `load_persisted_approval_store` after dropping expired entries, and by
/// `cleanup_expired_approvals`. The caller must hold `approval_file_lock`.
fn compact_approvals_unlocked(requests: &HashMap<Uuid, ApprovalRequest>) -> Result<()> {
    let path = approval_store_path()?;
    let mut entries: Vec<&ApprovalRequest> = requests.values().collect();
    entries.sort_by_key(|r| r.requested_at);
    let mut buf = String::new();
    for req in entries {
        buf.push_str(&serde_json::to_string(req)?);
        buf.push('\n');
    }
    // Temp-file + atomic rename, same pattern as save_config — readers never
    // see a half-written file.
    let tmp = path.with_extension(format!("jsonl.tmp.{}", std::process::id()));
    std::fs::write(&tmp, buf.as_bytes())?;
    crate::store::restrict_file_to_owner(&tmp)?;
    std::fs::rename(&tmp, &path)?;
    crate::store::restrict_file_to_owner(&path)?;
    Ok(())
}

/// Append a single approval entry to the JSONL store. Cheaper than
/// `persist_approval_store` (which rewrites the whole file) — this is O(1)
/// per write regardless of how many entries are on disk.
///
/// Because the file is JSONL, a later line with the same `id` naturally
/// supersedes the earlier line when read back (see `load_persisted_approval_store`).
/// This matches the access pattern: create a `Pending` request, then later
/// append an `Approved`/`Rejected`/`TimedOut` update for the same ID.
fn append_approval_unlocked(req: &ApprovalRequest) -> Result<()> {
    crate::store::ensure_config_dir()?;
    let path = approval_store_path()?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let line = serde_json::to_string(req)?;
    writeln!(file, "{line}")?;
    crate::store::restrict_file_to_owner(&path)?;
    Ok(())
}

/// Periodic cleanup: drop entries older than the retention window and compact
/// the file. Safe to call from a daemon background task on an interval (e.g.
/// once per hour). Acquires the file lock, so concurrent appends block briefly.
pub fn cleanup_expired_approvals() -> Result<()> {
    if !approval_persistence_enabled() {
        return Ok(());
    }
    let _guard = approval_file_lock()?;
    let path = approval_store_path()?;
    if !path.exists() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(&path)?;
    let mut kept: HashMap<Uuid, ApprovalRequest> = HashMap::new();
    let cutoff = Utc::now() - chrono::Duration::days(approval_retention_days());
    let mut dropped = 0;
    for line in raw.lines() {
        if let Ok(req) = serde_json::from_str::<ApprovalRequest>(line.trim()) {
            if req.requested_at > cutoff {
                kept.insert(req.id, req);
            } else {
                dropped += 1;
            }
        }
    }
    if dropped > 0 {
        compact_approvals_unlocked(&kept)?;
        let _ = crate::diagnostics::append_diagnostic_log_no_sink(
            "info",
            "approval",
            "periodic cleanup dropped expired approvals",
            Some(serde_json::json!({ "dropped": dropped, "kept": kept.len() })),
        );
    }
    Ok(())
}

/// Persist a single approval entry by appending it to the JSONL store. This
/// replaces the old `persist_approval_store(whole_store)` call pattern —
/// callers now append just the entry that changed, which is O(1) per write
/// instead of O(n) where n is the total approval history.
///
/// The caller is expected to have already mutated the in-memory store under
/// the tokio mutex; this function only handles the on-disk append.
fn append_approval(req: &ApprovalRequest) {
    if !approval_persistence_enabled() {
        return;
    }
    let result = (|| -> Result<()> {
        let _guard = approval_file_lock()?;
        append_approval_unlocked(req)
    })();
    if let Err(error) = result {
        let _ = crate::diagnostics::append_diagnostic_log(
            "warn",
            "approval",
            "failed to append approval entry",
            Some(serde_json::json!({ "id": req.id.to_string(), "error": error.to_string() })),
        );
    }
}

pub async fn approval_request(host: &str, command: &str, risk_level: RiskLevel) -> Uuid {
    approval_request_with_ttl(host, command, risk_level, DEFAULT_APPROVAL_TTL_SECS).await
}

/// Create an approval request with a custom TTL.
pub async fn approval_request_with_ttl(
    host: &str,
    command: &str,
    risk_level: RiskLevel,
    ttl_secs: u64,
) -> Uuid {
    let id = Uuid::new_v4();
    let req = ApprovalRequest {
        id,
        host: host.to_string(),
        command: command.to_string(),
        risk_level,
        requested_at: Utc::now(),
        ttl_secs,
        status: ApprovalStatus::Pending,
        context: None,
        revoked_at: None,
        approved_command_snapshot: None,
    };
    {
        let mut s = store().lock().await;
        s.requests.insert(id, req.clone());
        // Release the store lock before touching the file lock — otherwise
        // a slow disk write blocks every other approval_poll/list call.
        let to_persist = s.requests.get(&id).cloned();
        drop(s);
        if let Some(req) = to_persist {
            append_approval(&req);
        }
    }
    crate::events::publish_event(
        crate::events::EventType::ApprovalRequested,
        serde_json::json!({
            "id": id.to_string(),
            "host": host,
            "command": command,
            "risk_level": format!("{}", risk_level),
        }),
    );
    id
}

/// Create an approval request with rich context.
pub async fn approval_request_with_context(
    host: &str,
    command: &str,
    risk_level: RiskLevel,
    ttl_secs: u64,
    context: ApprovalContext,
) -> Uuid {
    let id = Uuid::new_v4();
    let req = ApprovalRequest {
        id,
        host: host.to_string(),
        command: command.to_string(),
        risk_level,
        requested_at: Utc::now(),
        ttl_secs,
        status: ApprovalStatus::Pending,
        context: Some(context),
        revoked_at: None,
        approved_command_snapshot: None,
    };
    {
        let mut s = store().lock().await;
        s.requests.insert(id, req.clone());
        let to_persist = s.requests.get(&id).cloned();
        drop(s);
        if let Some(req) = to_persist {
            append_approval(&req);
        }
    }
    crate::events::publish_event(
        crate::events::EventType::ApprovalRequested,
        serde_json::json!({
            "id": id.to_string(),
            "host": host,
            "command": command,
            "risk_level": format!("{}", risk_level),
        }),
    );
    id
}

pub async fn approval_poll(id: Uuid) -> Option<ApprovalStatus> {
    let (status, to_persist) = {
        let mut s = store().lock().await;
        let mut changed = false;
        let status = s.requests.get_mut(&id).map(|r| {
            if r.status == ApprovalStatus::Pending {
                let elapsed = Utc::now().signed_duration_since(r.requested_at);
                if elapsed.num_seconds() as u64 > r.ttl_secs {
                    r.status = ApprovalStatus::TimedOut;
                    changed = true;
                }
            }
            r.status
        });
        // Clone the entry that changed while we still hold the lock, then
        // release before the file write so other callers aren't blocked.
        let to_persist = if changed {
            s.requests.get(&id).cloned()
        } else {
            None
        };
        (status, to_persist)
    };
    if let Some(req) = to_persist {
        append_approval(&req);
    }
    status
}

pub async fn approval_respond(id: Uuid, approved: bool) -> Result<()> {
    let (status, to_persist) = {
        let mut s = store().lock().await;
        let req = s
            .requests
            .get_mut(&id)
            .ok_or_else(|| anyhow!("unknown approval: {id}"))?;

        // Check TTL expiration first, regardless of current status
        let elapsed = Utc::now().signed_duration_since(req.requested_at);
        if elapsed.num_seconds() as u64 > req.ttl_secs {
            if req.status == ApprovalStatus::Pending {
                req.status = ApprovalStatus::TimedOut;
            }
            // Even on TTL timeout we want to persist the TimedOut transition.
            let snapshot = req.clone();
            drop(s);
            append_approval(&snapshot);
            return Err(anyhow!("approval {id} has timed out"));
        }

        if req.status != ApprovalStatus::Pending {
            return Err(anyhow!("approval {id} already {:?}", req.status));
        }

        let status = if approved {
            // T2-14: Save command snapshot at approval time
            req.approved_command_snapshot = Some(req.command.clone());
            ApprovalStatus::Approved
        } else {
            ApprovalStatus::Rejected
        };
        req.status = status;
        let snapshot = req.clone();
        drop(s);
        (status, snapshot)
    };
    append_approval(&to_persist);
    crate::events::publish_event(
        crate::events::EventType::ApprovalResponded,
        serde_json::json!({
            "id": id.to_string(),
            "approved": approved,
            "status": format!("{:?}", status),
        }),
    );
    Ok(())
}

/// T2-14: Revoke a previously approved command.
///
/// This transitions an `Approved` approval to `Revoked`, recording the
/// revocation timestamp. The command snapshot is preserved so auditors
/// can verify what was approved before revocation.
///
/// Only `Approved` approvals can be revoked. Pending, Rejected, TimedOut,
/// or already Revoked approvals cannot be revoked.
pub async fn revoke_approval(id: Uuid) -> Result<()> {
    let to_persist = {
        let mut s = store().lock().await;
        let req = s
            .requests
            .get_mut(&id)
            .ok_or_else(|| anyhow!("unknown approval: {id}"))?;

        if req.status != ApprovalStatus::Approved {
            return Err(anyhow!(
                "approval {id} cannot be revoked: current status is {:?}",
                req.status
            ));
        }

        req.status = ApprovalStatus::Revoked;
        req.revoked_at = Some(Utc::now());
        let snapshot = req.clone();
        drop(s);
        snapshot
    };
    append_approval(&to_persist);
    crate::events::publish_event(
        crate::events::EventType::ApprovalResponded,
        serde_json::json!({
            "id": id.to_string(),
            "approved": false,
            "status": "revoked",
        }),
    );
    Ok(())
}

/// T2-14: Get the command snapshot from an approved request.
///
/// Returns `None` if the approval was never approved or if no snapshot
/// was taken. This is used to verify that the command being executed
/// matches what was actually approved.
pub async fn get_approved_command_snapshot(id: Uuid) -> Option<String> {
    let s = store().lock().await;
    s.requests
        .get(&id)
        .and_then(|req| req.approved_command_snapshot.clone())
}

pub async fn approval_list() -> Vec<ApprovalRequest> {
    let (result, to_persist) = {
        let mut s = store().lock().await;
        let now = Utc::now();
        let mut changed: Vec<ApprovalRequest> = Vec::new();
        for req in s.requests.values_mut() {
            if req.status == ApprovalStatus::Pending {
                let elapsed = now.signed_duration_since(req.requested_at);
                if elapsed.num_seconds() as u64 > req.ttl_secs {
                    req.status = ApprovalStatus::TimedOut;
                    changed.push(req.clone());
                }
            }
        }
        let result = s.requests.values().cloned().collect();
        (result, changed)
    };
    // Append each changed entry outside the store lock.
    for req in to_persist {
        append_approval(&req);
    }
    result
}

/// Wait for an approval to be resolved (approved/rejected/timed out).
/// Returns the final status.
pub async fn approval_wait(id: Uuid) -> ApprovalStatus {
    loop {
        let status = approval_poll(id).await;
        match status {
            Some(ApprovalStatus::Pending) => {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            Some(s) => return s,
            None => return ApprovalStatus::Rejected,
        }
    }
}

// ── Approval Context Builder ────────────────────────────────────────────────

/// Build an `ApprovalContext` by resolving host profile from config, fetching
/// recent audit entries for the host, and classifying risk with details.
pub fn build_approval_context(
    host_name: &str,
    command: &str,
    source: &str,
) -> Result<ApprovalContext> {
    let built_in_risk = crate::core::classify_risk(command);
    build_approval_context_with_effective_risk(host_name, command, source, built_in_risk, None)
}

pub fn build_approval_context_with_effective_risk(
    host_name: &str,
    command: &str,
    source: &str,
    final_risk: RiskLevel,
    matched_policy: Option<String>,
) -> Result<ApprovalContext> {
    use crate::types::AuditFilter;

    let mut ctx = ApprovalContext {
        source: Some(source.to_string()),
        ..Default::default()
    };

    // Resolve host profile from config
    let config = crate::store::load_config().unwrap_or_default();
    let mut risk_override = None;
    if let Some(profile) = config.hosts.iter().find(|h| h.name == host_name) {
        ctx.host_address = Some(profile.host.clone());
        ctx.host_env = profile.env.clone();
        ctx.host_role = profile.role.clone();
        risk_override = profile.risk_override;
    }

    // Get recent audit entries for this host (last 5)
    let filter = AuditFilter {
        host: Some(host_name.to_string()),
        limit: 5,
        ..Default::default()
    };
    if let Ok(entries) = crate::store::list_audit_raw(&filter) {
        ctx.recent_commands = entries
            .into_iter()
            .map(|e| ApprovalHistoryEntry {
                command: e.command,
                exit_code: e.exit_code,
                risk_level: e.risk_level,
                ts: e.ts.to_rfc3339(),
            })
            .collect();
    }

    // Classify risk with details
    let built_in_risk = crate::core::classify_risk(command);
    let risk_details = RiskDetails {
        built_in_risk,
        user_risk_override: risk_override,
        final_risk,
        matched_policy,
    };
    ctx.risk_details = Some(risk_details);

    Ok(ctx)
}

/// Generate an approval action URL for a given approval ID.
/// The URL points to the daemon's approval endpoint.
pub fn approval_action_url(daemon_url: &str, approval_id: &str) -> String {
    format!(
        "{}/approval/{}/respond",
        daemon_url.trim_end_matches('/'),
        approval_id
    )
}

// ── Approval Policy Configuration ────────────────────────────────────────────

/// Helper for serde default: `true`.
fn default_true() -> bool {
    true
}

/// Return a numeric ordinal for a `RiskLevel` so we can compare severity.
///
/// `Low(0) < Medium(1) < High(2) < Blocked(3)`
fn risk_ordinal(level: RiskLevel) -> u8 {
    match level {
        RiskLevel::Low => 0,
        RiskLevel::Medium => 1,
        RiskLevel::High => 2,
        RiskLevel::Blocked => 3,
    }
}

/// Simple glob matching supporting `*` (zero or more characters) and `?`
/// (exactly one character). Matching is case-insensitive.
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let text: Vec<char> = text.to_lowercase().chars().collect();
    glob_match_inner(&pattern, &text)
}

fn glob_match_inner(pattern: &[char], text: &[char]) -> bool {
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_pi, mut star_ti) = (usize::MAX, 0usize);

    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == '?' || pattern[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == '*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == '*' {
        pi += 1;
    }

    pi == pattern.len()
}

/// An approval policy rule that determines when approval is required.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalPolicy {
    /// Human-readable name for this policy
    pub name: String,
    /// Host names this policy applies to (empty = all hosts)
    #[serde(default)]
    pub hosts: Vec<String>,
    /// Tags — if any host tag matches, policy applies (empty = all tags)
    #[serde(default)]
    pub tags: Vec<String>,
    /// Minimum risk level that triggers approval (e.g., "high" means high+blocked need approval)
    #[serde(default)]
    pub min_risk: Option<RiskLevel>,
    /// Command pattern (glob-style: *, ?) — if command matches, policy applies
    #[serde(default)]
    pub command_pattern: Option<String>,
    /// Whether this policy requires approval (true) or auto-approves (false)
    #[serde(default = "default_true")]
    pub requires_approval: bool,
    /// Custom TTL for approvals created by this policy
    #[serde(default)]
    pub ttl_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApprovalPolicyFile {
    #[serde(default)]
    pub policies: Vec<ApprovalPolicy>,
}

/// Return the path to the approval policies TOML file.
fn approval_policies_path() -> PathBuf {
    crate::store::config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("approval_policies.toml")
}

/// Load approval policies from `~/.agent2ssh/approval_policies.toml`.
///
/// Returns an empty `Vec` if the file does not exist.
pub fn load_approval_policies() -> Result<Vec<ApprovalPolicy>> {
    if let Some(policies) = crate::policy::policy_approval_policies()? {
        return Ok(policies);
    }
    let path = approval_policies_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path)?;
    let file: ApprovalPolicyFile = toml::from_str(&raw)?;
    Ok(file.policies)
}

/// Save approval policies to `~/.agent2ssh/approval_policies.toml`.
pub fn save_approval_policies(policies: &[ApprovalPolicy]) -> Result<()> {
    if crate::policy::save_policy_approval_policies(policies)? {
        return Ok(());
    }
    let path = approval_policies_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = ApprovalPolicyFile {
        policies: policies.to_vec(),
    };
    let raw = toml::to_string_pretty(&file)?;
    std::fs::write(&path, raw)?;
    Ok(())
}

/// Check if a command on a host requires approval based on configured policies.
///
/// Returns the **first** matching `ApprovalPolicy` whose `requires_approval`
/// field is `true`, or `None` when no policy demands approval (auto-approve).
///
/// Matching rules (ALL must be true for a policy to match):
/// - `hosts` is empty **or** contains the host name (case-insensitive)
/// - `tags` is empty **or** any host tag matches any policy tag (case-insensitive)
/// - `min_risk` is `None` **or** the command's risk level >= min_risk
/// - `command_pattern` is `None` **or** the command matches the glob pattern
pub fn check_approval_required(
    host: &str,
    host_tags: &[String],
    command: &str,
    risk_level: RiskLevel,
) -> Result<Option<ApprovalPolicy>> {
    let policies = load_approval_policies()?;
    check_approval_required_with(&policies, host, host_tags, command, risk_level)
}

/// Core matching logic: evaluate a set of policies against a host/command/risk.
///
/// Returns the first matching policy that requires approval, or `None`.
fn check_approval_required_with(
    policies: &[ApprovalPolicy],
    host: &str,
    host_tags: &[String],
    command: &str,
    risk_level: RiskLevel,
) -> Result<Option<ApprovalPolicy>> {
    let host_lower = host.to_lowercase();

    for policy in policies {
        // Check hosts constraint
        if !policy.hosts.is_empty() && !policy.hosts.iter().any(|h| h.to_lowercase() == host_lower)
        {
            continue;
        }

        // Check tags constraint
        if !policy.tags.is_empty() {
            let policy_tags_lower: Vec<String> =
                policy.tags.iter().map(|t| t.to_lowercase()).collect();
            let any_tag_match = host_tags
                .iter()
                .any(|ht| policy_tags_lower.contains(&ht.to_lowercase()));
            if !any_tag_match {
                continue;
            }
        }

        // Check min_risk constraint
        if let Some(min_risk) = policy.min_risk {
            if risk_ordinal(risk_level) < risk_ordinal(min_risk) {
                continue;
            }
        }

        // Check command_pattern constraint
        if let Some(ref pattern) = policy.command_pattern {
            if !glob_match(pattern, command) {
                continue;
            }
        }

        // Policy matches
        if policy.requires_approval {
            return Ok(Some(policy.clone()));
        } else {
            // Explicitly auto-approves; stop evaluating
            return Ok(None);
        }
    }

    // No policy matched -> no approval required
    Ok(None)
}

/// List all configured approval policies.
pub fn list_approval_policies() -> Result<Vec<ApprovalPolicy>> {
    load_approval_policies()
}

#[cfg(test)]
mod tests {
    use super::*;

    // All approval tests share the process-global AppState.approvals store.
    // Mark them serial so concurrent test runs don't interleave mutations to
    // the same in-memory HashMap (which would make `approval_list` assertions
    // non-deterministic, even though each test's own approval IDs are unique).
    // Persistence is force-disabled in test builds via
    // `approval_persistence_enabled`, so these don't touch the real
    // approvals.json on disk.

    #[tokio::test]
    #[serial_test::serial]
    async fn test_approval_approve_flow() {
        let id = approval_request("testhost", "ls -la", RiskLevel::High).await;
        assert_eq!(approval_poll(id).await, Some(ApprovalStatus::Pending));
        approval_respond(id, true).await.unwrap();
        assert_eq!(approval_poll(id).await, Some(ApprovalStatus::Approved));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_approval_reject_flow() {
        let id = approval_request("testhost", "rm -rf /tmp", RiskLevel::High).await;
        assert_eq!(approval_poll(id).await, Some(ApprovalStatus::Pending));
        approval_respond(id, false).await.unwrap();
        assert_eq!(approval_poll(id).await, Some(ApprovalStatus::Rejected));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_approval_unknown_id() {
        let fake = Uuid::new_v4();
        assert_eq!(approval_poll(fake).await, None);
        assert!(approval_respond(fake, true).await.is_err());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_approval_double_respond() {
        let id = approval_request("testhost", "sudo whoami", RiskLevel::High).await;
        approval_respond(id, true).await.unwrap();
        assert!(approval_respond(id, false).await.is_err());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_approval_list_returns_all() {
        let id1 = approval_request("h1", "cmd1", RiskLevel::Medium).await;
        let id2 = approval_request("h2", "cmd2", RiskLevel::High).await;
        let list = approval_list().await;
        assert!(list.iter().any(|r| r.id == id1));
        assert!(list.iter().any(|r| r.id == id2));
    }

    // ── TTL behavior tests ──────────────────────────────────────────────────

    /// Approval created within TTL reports as Pending.
    #[tokio::test]
    #[serial_test::serial]
    async fn test_approval_within_ttl_is_pending() {
        let id = approval_request_with_ttl("host", "cmd", RiskLevel::High, 60).await;
        let status = approval_poll(id).await;
        assert_eq!(status, Some(ApprovalStatus::Pending));

        // Also verify via approval_list: should still be pending
        let list = approval_list().await;
        let entry = list.iter().find(|r| r.id == id).unwrap();
        assert_eq!(entry.status, ApprovalStatus::Pending);
    }

    /// Approval past TTL is reported as TimedOut.
    #[tokio::test]
    #[serial_test::serial]
    async fn test_approval_past_ttl_is_timed_out() {
        // Use a 1-second TTL and wait for it to expire
        let id = approval_request_with_ttl("host", "cmd", RiskLevel::High, 1).await;

        // Confirm it's pending immediately
        assert_eq!(approval_poll(id).await, Some(ApprovalStatus::Pending));

        // Wait for TTL to expire
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Now it should be TimedOut
        assert_eq!(approval_poll(id).await, Some(ApprovalStatus::TimedOut));
    }

    /// Approval list marks expired entries as TimedOut.
    #[tokio::test]
    #[serial_test::serial]
    async fn test_approval_list_marks_expired_as_timed_out() {
        let id = approval_request_with_ttl("host", "cmd", RiskLevel::High, 1).await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let list = approval_list().await;
        let entry = list.iter().find(|r| r.id == id).unwrap();
        assert_eq!(entry.status, ApprovalStatus::TimedOut);
    }

    /// Timed-out approval cannot be approved.
    #[tokio::test]
    #[serial_test::serial]
    async fn test_timed_out_approval_cannot_be_approved() {
        let id = approval_request_with_ttl("host", "cmd", RiskLevel::High, 1).await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let result = approval_respond(id, true).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("timed out"),
            "expected 'timed out' in error, got: {}",
            err_msg
        );
    }

    /// Timed-out approval cannot be rejected.
    #[tokio::test]
    #[serial_test::serial]
    async fn test_timed_out_approval_cannot_be_rejected() {
        let id = approval_request_with_ttl("host", "cmd", RiskLevel::High, 1).await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let result = approval_respond(id, false).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("timed out"),
            "expected 'timed out' in error, got: {}",
            err_msg
        );
    }

    /// Default TTL constant is 300 seconds.
    #[test]
    fn test_default_ttl_is_300() {
        assert_eq!(DEFAULT_APPROVAL_TTL_SECS, 300);
    }

    /// Default approval_request uses DEFAULT_APPROVAL_TTL_SECS.
    #[tokio::test]
    #[serial_test::serial]
    async fn test_default_approval_uses_default_ttl() {
        let id = approval_request("host", "cmd", RiskLevel::High).await;
        let s = store().lock().await;
        let req = s.requests.get(&id).unwrap();
        assert_eq!(req.ttl_secs, DEFAULT_APPROVAL_TTL_SECS);
    }

    // ── Approval Policy tests ────────────────────────────────────────────────

    #[test]
    fn test_approval_policy_toml_roundtrip() {
        let policies = vec![
            ApprovalPolicy {
                name: "production-high-risk".into(),
                hosts: vec!["prod-web-1".into(), "prod-db-1".into()],
                tags: vec!["production".into()],
                min_risk: Some(RiskLevel::High),
                command_pattern: Some("sudo *".into()),
                requires_approval: true,
                ttl_secs: Some(600),
            },
            ApprovalPolicy {
                name: "staging-auto".into(),
                hosts: vec![],
                tags: vec!["staging".into()],
                min_risk: None,
                command_pattern: None,
                requires_approval: false,
                ttl_secs: None,
            },
        ];

        let file = ApprovalPolicyFile {
            policies: policies.clone(),
        };
        let toml_str = toml::to_string_pretty(&file).expect("serialize to TOML");
        let parsed: ApprovalPolicyFile = toml::from_str(&toml_str).expect("deserialize from TOML");

        assert_eq!(parsed.policies.len(), 2);
        assert_eq!(parsed.policies[0].name, "production-high-risk");
        assert_eq!(parsed.policies[0].hosts.len(), 2);
        assert_eq!(parsed.policies[0].min_risk, Some(RiskLevel::High));
        assert_eq!(
            parsed.policies[0].command_pattern.as_deref(),
            Some("sudo *")
        );
        assert!(parsed.policies[0].requires_approval);
        assert_eq!(parsed.policies[0].ttl_secs, Some(600));

        assert_eq!(parsed.policies[1].name, "staging-auto");
        assert!(parsed.policies[1].hosts.is_empty());
        assert!(!parsed.policies[1].requires_approval);
    }

    #[test]
    fn test_check_approval_required_by_risk() {
        let policies = vec![ApprovalPolicy {
            name: "high-risk-only".into(),
            hosts: vec![],
            tags: vec![],
            min_risk: Some(RiskLevel::High),
            command_pattern: None,
            requires_approval: true,
            ttl_secs: Some(120),
        }];

        // High risk should require approval
        let result = check_approval_required_with(
            &policies,
            "any-host",
            &[],
            "sudo reboot",
            RiskLevel::High,
        )
        .unwrap();
        assert!(result.is_some(), "High risk should match min_risk=high");
        assert_eq!(result.unwrap().name, "high-risk-only");

        // Medium risk should NOT require approval
        let result = check_approval_required_with(
            &policies,
            "any-host",
            &[],
            "apt install",
            RiskLevel::Medium,
        )
        .unwrap();
        assert!(
            result.is_none(),
            "Medium risk should not match min_risk=high"
        );

        // Low risk should NOT require approval
        let result =
            check_approval_required_with(&policies, "any-host", &[], "ls -la", RiskLevel::Low)
                .unwrap();
        assert!(result.is_none(), "Low risk should not match min_risk=high");

        // Blocked risk should also match (>= high)
        let result = check_approval_required_with(
            &policies,
            "any-host",
            &[],
            "rm -rf /",
            RiskLevel::Blocked,
        )
        .unwrap();
        assert!(result.is_some(), "Blocked risk should match min_risk=high");
    }

    #[test]
    fn test_check_approval_required_by_host() {
        let policies = vec![ApprovalPolicy {
            name: "prod-only".into(),
            hosts: vec!["prod-web-1".into(), "PROD-DB-1".into()],
            tags: vec![],
            min_risk: None,
            command_pattern: None,
            requires_approval: true,
            ttl_secs: None,
        }];

        // Matching host (exact)
        let result =
            check_approval_required_with(&policies, "prod-web-1", &[], "ls", RiskLevel::Low)
                .unwrap();
        assert!(result.is_some(), "prod-web-1 should match");

        // Case-insensitive match
        let result =
            check_approval_required_with(&policies, "prod-db-1", &[], "ls", RiskLevel::Low)
                .unwrap();
        assert!(
            result.is_some(),
            "prod-db-1 should match PROD-DB-1 case-insensitively"
        );

        // Non-matching host
        let result =
            check_approval_required_with(&policies, "staging-1", &[], "ls", RiskLevel::Low)
                .unwrap();
        assert!(result.is_none(), "staging-1 should not match");
    }

    #[test]
    fn test_check_approval_required_by_tag() {
        let policies = vec![ApprovalPolicy {
            name: "production-tag".into(),
            hosts: vec![],
            tags: vec!["production".into(), "critical".into()],
            min_risk: None,
            command_pattern: None,
            requires_approval: true,
            ttl_secs: None,
        }];

        // Tag match
        let tags = vec!["production".to_string(), "web".to_string()];
        let result =
            check_approval_required_with(&policies, "any-host", &tags, "ls", RiskLevel::Low)
                .unwrap();
        assert!(result.is_some(), "production tag should match");

        // Tag match case-insensitive
        let tags = vec!["PRODUCTION".to_string()];
        let result =
            check_approval_required_with(&policies, "any-host", &tags, "ls", RiskLevel::Low)
                .unwrap();
        assert!(
            result.is_some(),
            "PRODUCTION tag should match case-insensitively"
        );

        // No matching tag
        let tags = vec!["staging".to_string()];
        let result =
            check_approval_required_with(&policies, "any-host", &tags, "ls", RiskLevel::Low)
                .unwrap();
        assert!(result.is_none(), "staging tag should not match");

        // Empty host tags should not match policy with tags
        let result =
            check_approval_required_with(&policies, "any-host", &[], "ls", RiskLevel::Low).unwrap();
        assert!(
            result.is_none(),
            "empty host tags should not match tag policy"
        );
    }

    #[test]
    fn test_check_approval_required_by_command_pattern() {
        let policies = vec![ApprovalPolicy {
            name: "dangerous-commands".into(),
            hosts: vec![],
            tags: vec![],
            min_risk: None,
            command_pattern: Some("kubectl delete *".into()),
            requires_approval: true,
            ttl_secs: None,
        }];

        // Matching command
        let result = check_approval_required_with(
            &policies,
            "any-host",
            &[],
            "kubectl delete namespace default",
            RiskLevel::Low,
        )
        .unwrap();
        assert!(result.is_some(), "kubectl delete ns should match pattern");

        // Non-matching command
        let result = check_approval_required_with(
            &policies,
            "any-host",
            &[],
            "kubectl get pods",
            RiskLevel::Low,
        )
        .unwrap();
        assert!(
            result.is_none(),
            "kubectl get pods should not match pattern"
        );
    }

    #[test]
    fn test_check_approval_not_required() {
        // Empty policies list
        let policies: Vec<ApprovalPolicy> = vec![];
        let result =
            check_approval_required_with(&policies, "any-host", &[], "ls -la", RiskLevel::Low)
                .unwrap();
        assert!(result.is_none(), "no policies should return None");
    }

    #[test]
    fn test_glob_match_basic() {
        // Exact match
        assert!(glob_match("hello", "hello"));
        assert!(!glob_match("hello", "world"));

        // Star wildcard
        assert!(glob_match(
            "kubectl delete *",
            "kubectl delete namespace default"
        ));
        assert!(!glob_match("kubectl delete *", "kubectl get pods"));
        assert!(glob_match("*", "anything at all"));
        assert!(glob_match("sudo *", "sudo apt update"));

        // Question mark
        assert!(glob_match("rm -r?", "rm -rf"));
        assert!(!glob_match("rm -r?", "rm -rf /"));

        // Combined
        assert!(glob_match(
            "git push * --force",
            "git push origin main --force"
        ));
        assert!(glob_match("?udo *", "sudo apt install"));

        // Case insensitive
        assert!(glob_match(
            "KUBECTL DELETE *",
            "kubectl delete namespace default"
        ));
        assert!(glob_match("kubectl *", "KUBECTL GET PODS"));
    }

    #[test]
    fn test_risk_ordinal_ordering() {
        assert!(risk_ordinal(RiskLevel::Low) < risk_ordinal(RiskLevel::Medium));
        assert!(risk_ordinal(RiskLevel::Medium) < risk_ordinal(RiskLevel::High));
        assert!(risk_ordinal(RiskLevel::High) < risk_ordinal(RiskLevel::Blocked));

        // Same level is equal
        assert_eq!(risk_ordinal(RiskLevel::Low), risk_ordinal(RiskLevel::Low));
        assert_eq!(
            risk_ordinal(RiskLevel::Blocked),
            risk_ordinal(RiskLevel::Blocked)
        );
    }

    // ── Approval Context tests ──────────────────────────────────────────────

    #[test]
    fn test_approval_context_serialization() {
        let ctx = ApprovalContext {
            host_address: Some("10.0.0.1".into()),
            host_env: Some("prod".into()),
            host_role: Some("web".into()),
            source: Some("cli".into()),
            recent_commands: vec![ApprovalHistoryEntry {
                command: "ls -la".into(),
                exit_code: Some(0),
                risk_level: RiskLevel::Low,
                ts: "2024-01-01T00:00:00Z".into(),
            }],
            risk_details: Some(RiskDetails {
                built_in_risk: RiskLevel::High,
                user_risk_override: None,
                final_risk: RiskLevel::High,
                matched_policy: Some("high-risk-only".into()),
            }),
            reason: Some("deploying hotfix".into()),
            change_id: Some("CHG-1234".into()),
        };

        let json = serde_json::to_string(&ctx).unwrap();
        let deserialized: ApprovalContext = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.host_address.as_deref(), Some("10.0.0.1"));
        assert_eq!(deserialized.host_env.as_deref(), Some("prod"));
        assert_eq!(deserialized.host_role.as_deref(), Some("web"));
        assert_eq!(deserialized.source.as_deref(), Some("cli"));
        assert_eq!(deserialized.recent_commands.len(), 1);
        assert_eq!(deserialized.recent_commands[0].command, "ls -la");
        assert_eq!(deserialized.reason.as_deref(), Some("deploying hotfix"));
        assert_eq!(deserialized.change_id.as_deref(), Some("CHG-1234"));
        let rd = deserialized.risk_details.unwrap();
        assert_eq!(rd.built_in_risk, RiskLevel::High);
        assert!(rd.user_risk_override.is_none());
        assert_eq!(rd.final_risk, RiskLevel::High);
        assert_eq!(rd.matched_policy.as_deref(), Some("high-risk-only"));
    }

    #[test]
    fn test_approval_context_default() {
        let ctx = ApprovalContext::default();
        assert!(ctx.host_address.is_none());
        assert!(ctx.host_env.is_none());
        assert!(ctx.host_role.is_none());
        assert!(ctx.source.is_none());
        assert!(ctx.recent_commands.is_empty());
        assert!(ctx.risk_details.is_none());
        assert!(ctx.reason.is_none());
        assert!(ctx.change_id.is_none());
    }

    #[test]
    fn test_approval_context_uses_effective_risk() {
        let ctx = build_approval_context_with_effective_risk(
            "missing-host",
            "sudo whoami",
            "daemon",
            RiskLevel::Low,
            Some("trusted-maintenance".into()),
        )
        .unwrap();

        let risk = ctx.risk_details.expect("risk details should be present");
        assert_eq!(risk.built_in_risk, RiskLevel::High);
        assert_eq!(risk.final_risk, RiskLevel::Low);
        assert_eq!(risk.matched_policy.as_deref(), Some("trusted-maintenance"));
    }

    #[tokio::test]
    async fn test_approval_request_with_context() {
        let ctx = ApprovalContext {
            host_address: Some("192.168.1.1".into()),
            host_env: Some("staging".into()),
            host_role: None,
            source: Some("mcp".into()),
            recent_commands: vec![],
            risk_details: None,
            reason: Some("testing".into()),
            change_id: None,
        };

        let id =
            approval_request_with_context("ctx-host", "sudo apt update", RiskLevel::High, 120, ctx)
                .await;

        // Verify it's stored with context
        let s = store().lock().await;
        let req = s.requests.get(&id).unwrap();
        assert_eq!(req.host, "ctx-host");
        assert_eq!(req.command, "sudo apt update");
        assert_eq!(req.ttl_secs, 120);
        assert!(req.context.is_some());
        let stored_ctx = req.context.as_ref().unwrap();
        assert_eq!(stored_ctx.host_address.as_deref(), Some("192.168.1.1"));
        assert_eq!(stored_ctx.host_env.as_deref(), Some("staging"));
        assert_eq!(stored_ctx.source.as_deref(), Some("mcp"));
        assert_eq!(stored_ctx.reason.as_deref(), Some("testing"));
    }

    #[test]
    fn test_approval_action_url() {
        let url = approval_action_url("http://127.0.0.1:7722", "abc-123-def");
        assert_eq!(url, "http://127.0.0.1:7722/approval/abc-123-def/respond");

        // Trailing slash should be trimmed
        let url = approval_action_url("http://127.0.0.1:7722/", "xyz-456");
        assert_eq!(url, "http://127.0.0.1:7722/approval/xyz-456/respond");
    }

    // ── T2-14: Command snapshot + revoke tests ────────────────────────────

    #[tokio::test]
    async fn t2_14_approved_command_snapshot_is_saved() {
        let id = approval_request("snap-host", "sudo reboot", RiskLevel::High).await;
        approval_respond(id, true).await.unwrap();

        // Snapshot should be saved
        let snapshot = get_approved_command_snapshot(id).await;
        assert_eq!(snapshot.as_deref(), Some("sudo reboot"));

        // Verify it's stored on the request
        let s = store().lock().await;
        let req = s.requests.get(&id).unwrap();
        assert!(req.approved_command_snapshot.is_some());
        assert_eq!(
            req.approved_command_snapshot.as_deref(),
            Some("sudo reboot")
        );
    }

    #[tokio::test]
    async fn t2_14_rejected_request_has_no_snapshot() {
        let id = approval_request("rej-host", "rm -rf /", RiskLevel::High).await;
        approval_respond(id, false).await.unwrap();

        let snapshot = get_approved_command_snapshot(id).await;
        assert!(
            snapshot.is_none(),
            "rejected request should have no snapshot"
        );
    }

    #[tokio::test]
    async fn t2_14_revoke_approved_approval() {
        let id = approval_request("rev-host", "shutdown -h now", RiskLevel::High).await;
        approval_respond(id, true).await.unwrap();
        assert_eq!(approval_poll(id).await, Some(ApprovalStatus::Approved));

        // Revoke it
        revoke_approval(id).await.unwrap();
        assert_eq!(approval_poll(id).await, Some(ApprovalStatus::Revoked));

        // Verify revoked_at is set
        let s = store().lock().await;
        let req = s.requests.get(&id).unwrap();
        assert!(req.revoked_at.is_some());
        // Snapshot should still be preserved
        assert_eq!(
            req.approved_command_snapshot.as_deref(),
            Some("shutdown -h now")
        );
    }

    #[tokio::test]
    async fn t2_14_revoke_non_approved_fails() {
        let id = approval_request("pend-host", "ls", RiskLevel::Medium).await;
        // Try to revoke a pending approval — should fail
        let result = revoke_approval(id).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("cannot be revoked"));
    }

    #[tokio::test]
    async fn t2_14_revoke_already_revoked_fails() {
        let id = approval_request("dblrev-host", "reboot", RiskLevel::High).await;
        approval_respond(id, true).await.unwrap();
        revoke_approval(id).await.unwrap();

        // Second revoke should fail
        let result = revoke_approval(id).await;
        assert!(result.is_err());
    }
}
