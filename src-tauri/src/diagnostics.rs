use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
use uuid::Uuid;

use crate::store::{config_dir, ensure_config_dir, redact_sensitive_text};

static DIAGNOSTIC_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn diagnostic_lock() -> &'static Mutex<()> {
    DIAGNOSTIC_LOCK.get_or_init(|| Mutex::new(()))
}

type DiagnosticErrorSink = Box<dyn Fn(&DiagnosticLogEntry) + Send + Sync>;
static ERROR_SINK: OnceLock<DiagnosticErrorSink> = OnceLock::new();

/// Register a callback invoked once for every `error`-level diagnostic entry,
/// after it has been written to `app.log` (and after the write lock is
/// released). Used by the daemon to forward error diagnostics to the notify
/// webhook for proactive alerting. The sink must not itself log at `error` level
/// or it risks feeding back into this path. First registration wins.
pub fn set_error_sink<F>(sink: F)
where
    F: Fn(&DiagnosticLogEntry) + Send + Sync + 'static,
{
    let _ = ERROR_SINK.set(Box::new(sink));
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
        previous(info);
    }));
}

pub fn append_diagnostic_log(
    level: &str,
    component: &str,
    message: &str,
    fields: Option<Value>,
) -> Result<DiagnosticLogEntry> {
    ensure_config_dir()?;
    let _guard = diagnostic_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("diagnostic log lock poisoned"))?;
    rotate_app_log_if_needed_unlocked(5 * 1024 * 1024)?;

    let entry = DiagnosticLogEntry {
        id: Uuid::new_v4().to_string(),
        ts: Utc::now().to_rfc3339(),
        level: normalize_level(level),
        component: component.trim().to_string(),
        message: redact_sensitive_text(message.trim()),
        fields: redact_value(fields.unwrap_or_else(|| json!({}))),
    };

    let path = app_log_path()?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open diagnostic log {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(&entry)?)?;

    // Release the write lock before notifying the sink so the callback (which may
    // log diagnostics of its own) cannot deadlock on the non-reentrant mutex.
    drop(_guard);
    if entry.level == "error" {
        if let Some(sink) = ERROR_SINK.get() {
            sink(&entry);
        }
    }
    Ok(entry)
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

        // OnceLock: first registration wins, so guard against other tests in the
        // same process already having set a sink.
        set_error_sink(|_entry| {
            SINK_HITS.fetch_add(1, Ordering::SeqCst);
        });
        assert!(ERROR_SINK.get().is_some(), "a sink must be registered");

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
}
