use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};
use uuid::Uuid;

use crate::app_state::app_state;
use crate::store::{config_dir, ensure_config_dir, redact_sensitive_text};

fn diagnostic_lock() -> &'static Mutex<()> {
    &app_state().diagnostic_lock
}

type DiagnosticErrorSink = Arc<dyn Fn(&DiagnosticLogEntry) + Send + Sync>;
// The error sink now lives in `AppState.error_sink` (RwLock<Option<Arc<...>>>)
// so it's co-located with all other process-wide state. The sink is stored
// behind an `Arc` so the write path can clone it out under a short read lock
// and invoke it without holding the lock — keeping the call site free of
// re-entrancy hazards if the sink itself logs. (H9)

fn error_sink() -> &'static std::sync::RwLock<Option<DiagnosticErrorSink>> {
    &app_state().error_sink
}

thread_local! {
    static CURRENT_TRACE_ID: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// Set (or clear with `None`) the correlation id for the current thread.
/// Diagnostic entries written from this thread are automatically tagged with a
/// `trace_id` field, so one logical operation can be followed across log lines
/// and across surfaces. Synchronous surfaces (CLI/MCP/Tauri) use this; the
/// daemon, being async, propagates its per-request id separately.
pub fn set_trace_id(trace_id: Option<String>) {
    CURRENT_TRACE_ID.with(|cell| *cell.borrow_mut() = trace_id);
}

/// Walk the full `std::error::Error::source()` chain and join all causes
/// into a single string, separated by `": "`.
///
/// Example: `"SSH handshake failed: key exchange error: no matching cipher"`
///
/// Used by the error sink and diagnostic logging to capture the complete
/// causal chain rather than just the top-level message.
pub fn error_chain(err: &dyn std::error::Error) -> String {
    let mut parts = vec![err.to_string()];
    let mut source = err.source();
    while let Some(s) = source {
        parts.push(s.to_string());
        source = s.source();
    }
    parts.join(": ")
}

/// The trace id bound to the current thread, if any.
pub fn current_trace_id() -> Option<String> {
    CURRENT_TRACE_ID.with(|cell| cell.borrow().clone())
}

/// Seed the current thread's trace id from `AGENT2SSH_TRACE_ID`, letting an
/// upstream caller (e.g. an agent shelling out to the CLI or MCP server)
/// propagate its own correlation id into Agent2SSH's diagnostics. Returns the
/// id that was applied, if any.
pub fn seed_trace_id_from_env() -> Option<String> {
    let id = std::env::var("AGENT2SSH_TRACE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());
    if id.is_some() {
        set_trace_id(id.clone());
    }
    id
}

/// Register a callback invoked once for every `error`-level diagnostic entry,
/// after it has been written to `app.log` (and after the write lock is
/// released). Used by the daemon to forward error diagnostics to the notify
/// webhook for proactive alerting. The sink must not itself log at `error` level
/// or it risks feeding back into this path.
///
/// Re-registration has explicit override semantics: the latest sink wins and a
/// `warn` diagnostic is recorded so a second initialization can never fail
/// silently. (H9)
pub fn set_error_sink<F>(sink: F)
where
    F: Fn(&DiagnosticLogEntry) + Send + Sync + 'static,
{
    let replaced = {
        let mut guard = error_sink()
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let replaced = guard.is_some();
        *guard = Some(Arc::new(sink));
        replaced
    };
    if replaced {
        // `warn` never re-enters the sink path (that only fires at `error`), so
        // surfacing the override here cannot loop. Recorded after the lock is
        // dropped above.
        let _ = append_diagnostic_log(
            "warn",
            "diagnostics",
            "error sink re-registered; replacing the previously installed sink",
            None,
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticLogEntry {
    pub id: String,
    pub ts: String,
    pub level: String,
    pub component: String,
    pub message: String,
    #[serde(default)]
    pub fields: Value,
}

pub fn app_log_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("app.log"))
}

pub fn diagnostic_bundle_path() -> Result<PathBuf> {
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    Ok(config_dir()?.join(format!("diagnostics-{stamp}.txt")))
}

fn diagnostic_error_alert_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(".diagnostic_error_alert"))
}

fn normalize_level(level: &str) -> String {
    match level.trim().to_ascii_lowercase().as_str() {
        "trace" | "debug" | "info" | "warn" | "error" => level.trim().to_ascii_lowercase(),
        _ => "info".into(),
    }
}

fn redact_value(value: Value) -> Value {
    match value {
        Value::String(text) => Value::String(redact_sensitive_text(&text)),
        Value::Array(values) => Value::Array(values.into_iter().map(redact_value).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let key_lower = key.to_ascii_lowercase();
                    if matches!(
                        key_lower.as_str(),
                        "password"
                            | "passwd"
                            | "token"
                            | "secret"
                            | "api_key"
                            | "apikey"
                            | "authorization"
                            | "cookie"
                    ) {
                        (key, Value::String("[REDACTED]".into()))
                    } else {
                        (key, redact_value(value))
                    }
                })
                .collect(),
        ),
        other => other,
    }
}

fn rotate_app_log_if_needed_unlocked(max_size_bytes: u64) -> Result<()> {
    let path = app_log_path()?;
    if !path.exists() {
        return Ok(());
    }
    let metadata = std::fs::metadata(&path)?;
    if metadata.len() <= max_size_bytes {
        return Ok(());
    }

    for i in (2..=3).rev() {
        let src = path.with_extension(format!("log.{}", i - 1));
        let dst = path.with_extension(format!("log.{i}"));
        if src.exists() {
            if dst.exists() {
                std::fs::remove_file(&dst)?;
            }
            std::fs::rename(&src, &dst)?;
        }
    }

    let rotated = path.with_extension("log.1");
    if rotated.exists() {
        std::fs::remove_file(&rotated)?;
    }
    std::fs::rename(&path, &rotated)?;
    Ok(())
}

/// Install a process-wide panic hook that records every panic to the diagnostic
/// log (`app.log`) before delegating to the previous hook (so the default
/// stderr/backtrace behavior is preserved). Idempotent per process — call once
/// at the start of `main`/`run_tauri`. This makes otherwise-silent thread panics
/// observable across all four surfaces, not just whatever happens to capture
/// stderr.
pub fn install_panic_hook(component: &'static str) {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    if INSTALLED.set(()).is_err() {
        // A hook is already installed; chaining another would double-record every
        // panic. Make the duplicate call explicit instead of silently ignoring it
        // so a stray second init is observable. (H9)
        let _ = append_diagnostic_log(
            "warn",
            component,
            "panic hook already installed; ignoring duplicate install",
            None,
        );
        return;
    }

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = match info.payload().downcast_ref::<&str>() {
            Some(text) => (*text).to_string(),
            None => match info.payload().downcast_ref::<String>() {
                Some(text) => text.clone(),
                None => "panic with non-string payload".to_string(),
            },
        };
        let location = info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()));
        let _ = append_diagnostic_log(
            "error",
            component,
            &format!("panic: {message}"),
            Some(json!({ "location": location })),
        );
        // K10: opt-in crash aggregation (no-op unless the user enabled telemetry).
        crate::telemetry::record_event(
            "crash",
            json!({ "component": component, "message": message, "location": location }),
        );
        previous(info);
    }));
}

pub fn append_diagnostic_log(
    level: &str,
    component: &str,
    message: &str,
    fields: Option<Value>,
) -> Result<DiagnosticLogEntry> {
    append_diagnostic_log_inner(level, component, message, fields, true)
}

/// Like [`append_diagnostic_log`] but never fans the entry out to the error
/// sink, even at `error` level. Used by the daemon's dependency-layer log bridge
/// so third-party `hyper`/`reqwest`/`ssh2` warnings/errors stay observable in
/// `app.log` without re-triggering the error-alert webhook — which itself talks
/// over `reqwest` and could otherwise loop a transport error back into more
/// transport errors. (H7 anti-loop)
pub fn append_diagnostic_log_no_sink(
    level: &str,
    component: &str,
    message: &str,
    fields: Option<Value>,
) -> Result<DiagnosticLogEntry> {
    append_diagnostic_log_inner(level, component, message, fields, false)
}

fn append_diagnostic_log_inner(
    level: &str,
    component: &str,
    message: &str,
    fields: Option<Value>,
    notify_sink: bool,
) -> Result<DiagnosticLogEntry> {
    ensure_config_dir()?;
    // Two-tier lock matching store/audit: a process-local mutex serializes our
    // own threads, and a cross-process advisory file lock keeps the other
    // surfaces (CLI/MCP/daemon/desktop) from interleaving appends or racing a
    // rotation on the shared app.log.
    let _guard = diagnostic_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("diagnostic log lock poisoned"))?;
    let _file_lock = crate::store::lock_config_file(".app_log.lock")?;
    rotate_app_log_if_needed_unlocked(5 * 1024 * 1024)?;

    let mut fields = redact_value(fields.unwrap_or_else(|| json!({})));
    // Tag the entry with the current thread's correlation id (unless the caller
    // already supplied one explicitly) so an operation can be traced end to end.
    if let (Some(trace_id), Value::Object(map)) = (current_trace_id(), &mut fields) {
        map.entry("trace_id".to_string())
            .or_insert(Value::String(trace_id));
    }

    let entry = DiagnosticLogEntry {
        id: Uuid::new_v4().to_string(),
        ts: Utc::now().to_rfc3339(),
        level: normalize_level(level),
        component: component.trim().to_string(),
        message: redact_sensitive_text(message.trim()),
        fields,
    };

    let path = app_log_path()?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open diagnostic log {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(&entry)?)?;
    let diagnostic_findings = if entry.level == "error" {
        diagnostic_error_findings_from_app_log(&entry)
    } else {
        Vec::new()
    };

    // Release both locks before notifying the sink so the callback (which may log
    // diagnostics of its own) cannot deadlock on the non-reentrant mutex or the
    // exclusive file lock.
    drop(_file_lock);
    drop(_guard);
    crate::anomaly::publish_anomalies(&diagnostic_findings);
    if notify_sink && entry.level == "error" {
        // Clone the sink out under a short read lock and release it before
        // invoking, so a sink that logs cannot deadlock against a concurrent
        // re-registration or recursively read-lock on this thread.
        let sink = error_sink().read().ok().and_then(|guard| guard.clone());
        if let Some(sink) = sink {
            sink(&entry);
        }
    }
    Ok(entry)
}

fn diagnostic_error_findings_from_app_log(
    current: &DiagnosticLogEntry,
) -> Vec<crate::anomaly::AnomalyFinding> {
    let config = crate::anomaly::load_anomaly_config().unwrap_or_default();
    if !config.enabled || config.diagnostic_error_threshold == 0 {
        return Vec::new();
    }
    if current.message.to_lowercase().contains("webhook") {
        return Vec::new();
    }

    let Ok(current_ts) =
        chrono::DateTime::parse_from_rfc3339(&current.ts).map(|ts| ts.with_timezone(&chrono::Utc))
    else {
        return Vec::new();
    };
    let window = chrono::Duration::seconds(config.window_secs.max(1));
    let since = current_ts - window;
    let Ok(path) = app_log_path() else {
        return Vec::new();
    };
    let raw = std::fs::read_to_string(&path).unwrap_or_default();
    let count = raw
        .lines()
        .filter_map(|line| serde_json::from_str::<DiagnosticLogEntry>(line).ok())
        .filter(|entry| entry.level == "error")
        .filter(|entry| !entry.message.to_lowercase().contains("webhook"))
        .filter_map(|entry| {
            chrono::DateTime::parse_from_rfc3339(&entry.ts)
                .ok()
                .map(|ts| ts.with_timezone(&chrono::Utc))
        })
        .filter(|ts| *ts >= since && *ts <= current_ts)
        .count();
    if count < config.diagnostic_error_threshold {
        return Vec::new();
    }

    let cooldown = chrono::Duration::seconds(config.diagnostic_cooldown_secs.max(1));
    let Ok(alert_path) = diagnostic_error_alert_path() else {
        return Vec::new();
    };
    if let Ok(raw) = std::fs::read_to_string(&alert_path) {
        if let Ok(last) = chrono::DateTime::parse_from_rfc3339(raw.trim())
            .map(|ts| ts.with_timezone(&chrono::Utc))
        {
            if current_ts - last < cooldown {
                return Vec::new();
            }
        }
    }
    if std::fs::write(&alert_path, current_ts.to_rfc3339()).is_ok() {
        let _ = crate::store::restrict_file_to_owner(&alert_path);
    }

    vec![crate::anomaly::AnomalyFinding {
        kind: crate::anomaly::AnomalyKind::DiagnosticErrorBurst,
        severity: crate::anomaly::AnomalySeverity::High,
        reason: format!(
            "{count} error-level diagnostics in {}s (latest component: {})",
            config.window_secs, current.component
        ),
        source: current.component.clone(),
        host: "local".to_string(),
        command: current.message.chars().take(200).collect(),
        count,
        threshold: config.diagnostic_error_threshold,
        window_secs: config.window_secs,
    }]
}

pub fn list_diagnostic_logs(limit: usize) -> Result<Vec<DiagnosticLogEntry>> {
    let path = app_log_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read diagnostic log {}", path.display()))?;
    let mut entries: Vec<DiagnosticLogEntry> = raw
        .lines()
        .filter_map(|line| serde_json::from_str::<DiagnosticLogEntry>(line).ok())
        .collect();
    entries.reverse();
    entries.truncate(limit.clamp(1, 1000));
    Ok(entries)
}

pub fn clear_diagnostic_logs() -> Result<()> {
    ensure_config_dir()?;
    let _guard = diagnostic_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("diagnostic log lock poisoned"))?;
    let _file_lock = crate::store::lock_config_file(".app_log.lock")?;
    let path = app_log_path()?;
    std::fs::write(&path, "").with_context(|| format!("failed to clear {}", path.display()))?;
    Ok(())
}

fn read_tail(path: PathBuf, max_lines: usize) -> String {
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return format!("unavailable: {}", path.display());
    };
    let mut lines: Vec<&str> = raw.lines().rev().take(max_lines).collect();
    lines.reverse();
    lines
        .into_iter()
        .map(redact_sensitive_text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn file_metadata(path: PathBuf) -> Value {
    match std::fs::metadata(&path) {
        Ok(metadata) => json!({
            "path": path.display().to_string(),
            "bytes": metadata.len(),
            "modified": metadata.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())),
        }),
        Err(error) => json!({
            "path": path.display().to_string(),
            "error": error.to_string(),
        }),
    }
}

pub fn export_diagnostic_bundle() -> Result<PathBuf> {
    ensure_config_dir()?;
    let config_dir = config_dir()?;
    let app_log = app_log_path()?;
    let daemon_log = config_dir.join("daemon.log");
    let audit_log = config_dir.join("audit.jsonl");
    let hosts_config = config_dir.join("hosts.json");
    let gate_config = config_dir.join("execution_gate.json");
    let daemon_pid = config_dir.join("daemon.pid");

    let pid = crate::daemon_control::read_daemon_pid().ok().flatten();
    let health_ok = crate::daemon_control::daemon_health_ok();
    let summary = json!({
        "generated_at": Utc::now().to_rfc3339(),
        "config_dir": config_dir.display().to_string(),
        "daemon_pid": pid,
        "daemon_process_alive": pid.map(crate::daemon_control::process_is_alive),
        "daemon_health_ok": health_ok,
        "files": [
            file_metadata(app_log.clone()),
            file_metadata(daemon_log.clone()),
            file_metadata(audit_log.clone()),
            file_metadata(hosts_config),
            file_metadata(gate_config),
            file_metadata(daemon_pid),
        ],
    });

    let mut body = String::new();
    body.push_str("# Agent2SSH diagnostics\n\n");
    body.push_str("## Summary\n");
    body.push_str(&serde_json::to_string_pretty(&summary)?);
    body.push_str("\n\n## app.log tail\n");
    body.push_str(&read_tail(app_log, 300));
    body.push_str("\n\n## daemon.log tail\n");
    body.push_str(&read_tail(daemon_log, 300));
    body.push_str("\n\n## audit.jsonl tail\n");
    body.push_str(&read_tail(audit_log, 120));
    body.push('\n');

    let out = diagnostic_bundle_path()?;
    std::fs::write(&out, body)
        .with_context(|| format!("failed to write diagnostic bundle {}", out.display()))?;
    Ok(out)
}

// ── B56: Structured Backend System Report ───────────────────────────────────

/// Generate a structured JSON system report combining local system metrics,
/// application state, config overview, and health snapshot data.
pub fn generate_system_report() -> Result<serde_json::Value> {
    let config_dir = config_dir()?;

    // 1. Local system metrics via sysinfo
    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();

    let total_memory_gb = sys.total_memory() as f64 / 1_073_741_824.0;
    let used_memory_gb = sys.used_memory() as f64 / 1_073_741_824.0;
    let available_memory_gb = sys.available_memory() as f64 / 1_073_741_824.0;

    let cpus: Vec<serde_json::Value> = sys
        .cpus()
        .iter()
        .take(4) // limit to first 4 cores to keep report compact
        .map(|cpu| {
            json!({
                "name": cpu.name().to_string(),
                "usage_pct": cpu.cpu_usage(),
                "frequency_mhz": cpu.frequency(),
            })
        })
        .collect();

    // 2. Application state
    let host = crate::app_state::host();
    let transport = host.transport_name().to_string();
    let is_desktop = host.is_desktop();
    drop(host);

    // 3. Lifecycle resources
    let lifecycle = crate::app_state::lifecycle();
    let lifecycle_summary = lifecycle
        .lock()
        .map(|reg| {
            let ready_list = reg.list_by_phase(crate::app_state::ResourcePhase::Ready);
            let pending_list = reg.list_by_phase(crate::app_state::ResourcePhase::Pending);
            json!({
                "active_resources": ready_list.len() + pending_list.len(),
                "ready": ready_list.len(),
                "pending": pending_list.len(),
            })
        })
        .unwrap_or_else(|_| json!({}));

    // 4. Config overview
    let config_overview = match crate::store::load_config() {
        Ok(config) => {
            json!({
                "host_count": config.hosts.len(),
                "proxy_count": config.proxies.len(),
                "host_groups": config.groups.len(),
            })
        }
        Err(e) => json!({ "error": e.to_string() }),
    };

    // 5. Daemon status
    let daemon_pid = crate::daemon_control::read_daemon_pid().ok().flatten();
    let daemon_alive = daemon_pid.map(crate::daemon_control::process_is_alive);
    let daemon_health_ok = crate::daemon_control::daemon_health_ok();

    // 6. Health snapshot
    let health_snapshot = crate::health::load_health_snapshot()
        .map(|h| serde_json::to_value(&h).unwrap_or(json!(null)))
        .unwrap_or(json!(null));

    // 7. Recent diagnostic log entries
    let recent_logs = list_diagnostic_logs(20).unwrap_or_default();
    let log_entries: Vec<serde_json::Value> = recent_logs
        .iter()
        .map(|entry| {
            json!({
                "ts": entry.ts,
                "level": entry.level,
                "component": entry.component,
                "message": entry.message,
            })
        })
        .collect();

    Ok(json!({
        "generated_at": Utc::now().to_rfc3339(),
        "version": crate::remote::PROTOCOL_VERSION,
        "transport": transport,
        "is_desktop": is_desktop,
        "system": {
            "platform": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "hostname": sysinfo::System::host_name().unwrap_or_default(),
            "uptime_secs": sysinfo::System::uptime(),
            "cpu": {
                "core_count": sys.cpus().len(),
                "global_usage_pct": sys.global_cpu_usage(),
                "cores": cpus,
            },
            "memory": {
                "total_gb": (total_memory_gb * 100.0).round() / 100.0,
                "used_gb": (used_memory_gb * 100.0).round() / 100.0,
                "available_gb": (available_memory_gb * 100.0).round() / 100.0,
            },
        },
        "application": {
            "config_dir": config_dir.display().to_string(),
            "config": config_overview,
            "lifecycle": lifecycle_summary,
        },
        "daemon": {
            "pid": daemon_pid,
            "process_alive": daemon_alive,
            "health_ok": daemon_health_ok,
        },
        "health_snapshot": health_snapshot,
        "recent_logs": log_entries,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SINK_HITS: AtomicUsize = AtomicUsize::new(0);

    #[test]
    #[serial_test::serial]
    fn error_sink_fires_only_for_errors_and_redacts() {
        let config_dir = std::env::temp_dir().join(format!("agent2ssh-diag-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&config_dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &config_dir);

        // Re-registration overrides (H9): this test's sink replaces whatever an
        // earlier test installed, so the hit counter below reflects only us.
        set_error_sink(|_entry| {
            SINK_HITS.fetch_add(1, Ordering::SeqCst);
        });
        assert!(
            error_sink().read().unwrap().is_some(),
            "a sink must be registered"
        );

        let before = SINK_HITS.load(Ordering::SeqCst);
        append_diagnostic_log("info", "test", "an info message", None).unwrap();
        let after_info = SINK_HITS.load(Ordering::SeqCst);

        append_diagnostic_log(
            "error",
            "test",
            "boom",
            Some(json!({ "password": "hunter2" })),
        )
        .unwrap();
        let after_error = SINK_HITS.load(Ordering::SeqCst);

        // Only the error entry should reach the sink (delta of exactly 1).
        assert_eq!(after_info, before, "info must not trigger the error sink");
        assert_eq!(after_error, before + 1, "error must trigger the error sink");

        // Secret fields must be redacted before they ever hit disk.
        let logs = list_diagnostic_logs(10).unwrap();
        let entry = logs
            .iter()
            .find(|e| e.component == "test" && e.level == "error")
            .expect("error entry should be persisted");
        assert_ne!(
            entry.fields["password"], "hunter2",
            "password field must be redacted"
        );

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&config_dir);
    }

    #[test]
    #[serial_test::serial]
    fn no_sink_variant_writes_but_never_fans_out() {
        let config_dir = std::env::temp_dir().join(format!("agent2ssh-diag-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&config_dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &config_dir);

        set_error_sink(|_entry| {
            SINK_HITS.fetch_add(1, Ordering::SeqCst);
        });

        let before = SINK_HITS.load(Ordering::SeqCst);
        // An error written via the no-sink variant must still land in app.log...
        append_diagnostic_log_no_sink("error", "deps", "transport boom", None).unwrap();
        let after = SINK_HITS.load(Ordering::SeqCst);

        // ...but must not reach the error sink (H7 anti-loop).
        assert_eq!(
            after, before,
            "no-sink variant must not trigger the error sink"
        );
        let logs = list_diagnostic_logs(10).unwrap();
        assert!(
            logs.iter()
                .any(|e| e.component == "deps" && e.level == "error"),
            "no-sink error entry should still be persisted"
        );

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&config_dir);
    }

    #[test]
    #[serial_test::serial]
    fn diagnostic_error_burst_uses_shared_app_log_window() {
        let config_dir =
            std::env::temp_dir().join(format!("agent2ssh-diag-burst-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&config_dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &config_dir);
        std::fs::write(
            config_dir.join("anomaly.toml"),
            r#"
enabled = true
window_secs = 60
diagnostic_error_threshold = 3
diagnostic_cooldown_secs = 60
"#,
        )
        .unwrap();

        append_diagnostic_log("error", "cli", "cli failed", None).unwrap();
        append_diagnostic_log("error", "mcp", "mcp failed", None).unwrap();
        append_diagnostic_log("error", "tauri", "tauri failed", None).unwrap();

        let alert_path = diagnostic_error_alert_path().unwrap();
        let first_alert = std::fs::read_to_string(&alert_path)
            .expect("error burst should update shared cooldown marker");
        assert!(
            chrono::DateTime::parse_from_rfc3339(first_alert.trim()).is_ok(),
            "cooldown marker should contain an RFC3339 timestamp"
        );

        append_diagnostic_log("error", "daemon", "webhook delivery failed", None).unwrap();
        let after_webhook = std::fs::read_to_string(&alert_path).unwrap();
        assert_eq!(
            first_alert, after_webhook,
            "webhook errors must not refresh diagnostic burst cooldown"
        );

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&config_dir);
    }

    #[test]
    fn error_chain_walks_full_cause_chain() {
        use std::fmt;

        #[derive(Debug)]
        struct InnerError(&'static str);
        impl fmt::Display for InnerError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
        impl std::error::Error for InnerError {}

        #[derive(Debug)]
        struct MiddleError(&'static str, InnerError);
        impl fmt::Display for MiddleError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
        impl std::error::Error for MiddleError {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.1)
            }
        }

        #[derive(Debug)]
        struct OuterError(&'static str, MiddleError);
        impl fmt::Display for OuterError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
        impl std::error::Error for OuterError {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.1)
            }
        }

        let err = OuterError("outer", MiddleError("middle", InnerError("inner")));
        let chain = error_chain(&err);
        assert_eq!(chain, "outer: middle: inner");
    }

    #[test]
    fn error_chain_single_level_no_source() {
        use std::fmt;
        #[derive(Debug)]
        struct SimpleError(&'static str);
        impl fmt::Display for SimpleError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
        impl std::error::Error for SimpleError {}

        let err = SimpleError("just one level");
        let chain = error_chain(&err);
        assert_eq!(chain, "just one level");
    }
}
