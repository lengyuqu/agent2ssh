use crate::events::{publish_event, EventType};
use crate::types::{AuditEntry, RiskLevel};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_window_secs")]
    pub window_secs: i64,
    #[serde(default = "default_source_burst_threshold")]
    pub source_burst_threshold: usize,
    #[serde(default = "default_sensitive_threshold")]
    pub sensitive_threshold: usize,
    #[serde(default = "default_sensitive_patterns")]
    pub sensitive_patterns: Vec<String>,
    #[serde(default = "default_after_hours_start")]
    pub after_hours_start: u32,
    #[serde(default = "default_after_hours_end")]
    pub after_hours_end: u32,
    #[serde(default = "default_after_hours_risks")]
    pub after_hours_risks: Vec<RiskLevel>,
    #[serde(default = "default_diagnostic_error_threshold")]
    pub diagnostic_error_threshold: usize,
    #[serde(default = "default_diagnostic_cooldown_secs")]
    pub diagnostic_cooldown_secs: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnomalyKind {
    SourceBurst,
    SensitivePattern,
    AfterHours,
    DiagnosticErrorBurst,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnomalySeverity {
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyFinding {
    pub kind: AnomalyKind,
    pub severity: AnomalySeverity,
    pub reason: String,
    pub source: String,
    pub host: String,
    pub command: String,
    pub count: usize,
    pub threshold: usize,
    pub window_secs: i64,
}

impl Default for AnomalyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            window_secs: default_window_secs(),
            source_burst_threshold: default_source_burst_threshold(),
            sensitive_threshold: default_sensitive_threshold(),
            sensitive_patterns: default_sensitive_patterns(),
            after_hours_start: default_after_hours_start(),
            after_hours_end: default_after_hours_end(),
            after_hours_risks: default_after_hours_risks(),
            diagnostic_error_threshold: default_diagnostic_error_threshold(),
            diagnostic_cooldown_secs: default_diagnostic_cooldown_secs(),
        }
    }
}

static ANOMALY_CACHE: crate::config_cache::ConfigCache<AnomalyConfig> =
    crate::config_cache::ConfigCache::new();

pub fn load_anomaly_config() -> Result<AnomalyConfig> {
    let path = anomaly_config_path()?;
    ANOMALY_CACHE.load_with(&path, || {
        if !path.exists() {
            return Ok(AnomalyConfig::default());
        }
        let raw = fs::read_to_string(&path)?;
        Ok(toml::from_str(&raw)?)
    })
}

pub fn detect_anomalies(
    entries: &[AuditEntry],
    current: &AuditEntry,
    config: &AnomalyConfig,
) -> Vec<AnomalyFinding> {
    if !config.enabled {
        return Vec::new();
    }

    let source = current.source.as_deref().unwrap_or("unknown").to_string();
    let since = current.ts - chrono::Duration::seconds(config.window_secs.max(1));
    let window: Vec<&AuditEntry> = entries
        .iter()
        .filter(|entry| entry.ts >= since && entry.ts <= current.ts)
        .collect();

    let mut findings = Vec::new();

    let source_count = window
        .iter()
        .filter(|entry| entry.source.as_deref().unwrap_or("unknown") == source)
        .count();
    if config.source_burst_threshold > 0 && source_count >= config.source_burst_threshold {
        findings.push(AnomalyFinding {
            kind: AnomalyKind::SourceBurst,
            severity: AnomalySeverity::High,
            reason: format!(
                "source {source} executed {source_count} commands in {}s",
                config.window_secs
            ),
            source: source.clone(),
            host: current.host.clone(),
            command: current.command.clone(),
            count: source_count,
            threshold: config.source_burst_threshold,
            window_secs: config.window_secs,
        });
    }

    let current_command = current.command.to_lowercase();
    let sensitive_matched = config
        .sensitive_patterns
        .iter()
        .any(|pattern| command_matches(&current_command, pattern));
    if sensitive_matched {
        let sensitive_count = window
            .iter()
            .filter(|entry| {
                let command = entry.command.to_lowercase();
                config
                    .sensitive_patterns
                    .iter()
                    .any(|pattern| command_matches(&command, pattern))
            })
            .count();
        if config.sensitive_threshold > 0 && sensitive_count >= config.sensitive_threshold {
            findings.push(AnomalyFinding {
                kind: AnomalyKind::SensitivePattern,
                severity: AnomalySeverity::High,
                reason: format!(
                    "sensitive command pattern seen {sensitive_count} times in {}s",
                    config.window_secs
                ),
                source: source.clone(),
                host: current.host.clone(),
                command: current.command.clone(),
                count: sensitive_count,
                threshold: config.sensitive_threshold,
                window_secs: config.window_secs,
            });
        }
    }

    if is_after_hours(
        current.ts.hour(),
        config.after_hours_start,
        config.after_hours_end,
    ) && config
        .after_hours_risks
        .iter()
        .any(|risk| risk == &current.risk_level)
    {
        findings.push(AnomalyFinding {
            kind: AnomalyKind::AfterHours,
            severity: AnomalySeverity::Medium,
            reason: format!(
                "{} risk command during after-hours window {:02}:00-{:02}:00 UTC",
                current.risk_level, config.after_hours_start, config.after_hours_end
            ),
            source,
            host: current.host.clone(),
            command: current.command.clone(),
            count: 1,
            threshold: 1,
            window_secs: config.window_secs,
        });
    }

    dedupe_findings(findings)
}

use std::collections::VecDeque;
use std::sync::Mutex as StdMutex;

static ERROR_TIMES: std::sync::OnceLock<StdMutex<VecDeque<chrono::DateTime<chrono::Utc>>>> =
    std::sync::OnceLock::new();
static LAST_ERROR_ALERT: std::sync::OnceLock<StdMutex<Option<chrono::DateTime<chrono::Utc>>>> =
    std::sync::OnceLock::new();

/// Feed one `error`-level diagnostic into a process-local sliding window and
/// return an [`AnomalyFinding`] when the error rate crosses
/// `diagnostic_error_threshold` within `window_secs`. A `diagnostic_cooldown_secs`
/// gate prevents repeat alerts while a burst persists, so this complements the
/// per-error webhook with a single aggregate signal. The daemon wires its
/// diagnostic error sink to this; callers pass the result to [`publish_anomalies`].
pub fn record_diagnostic_error(component: &str, message: &str) -> Vec<AnomalyFinding> {
    let config = load_anomaly_config().unwrap_or_default();
    if !config.enabled || config.diagnostic_error_threshold == 0 {
        return Vec::new();
    }
    // Ignore internal webhook-delivery failures so a misconfigured alert endpoint
    // cannot inflate — and thereby self-trigger — the error-burst counter.
    if message.to_lowercase().contains("webhook") {
        return Vec::new();
    }

    let now = chrono::Utc::now();
    let window = chrono::Duration::seconds(config.window_secs.max(1));
    let times = ERROR_TIMES.get_or_init(|| StdMutex::new(VecDeque::new()));
    let count = {
        let mut times = times.lock().unwrap_or_else(|p| p.into_inner());
        times.push_back(now);
        while times.front().is_some_and(|front| now - *front > window) {
            times.pop_front();
        }
        times.len()
    };
    if count < config.diagnostic_error_threshold {
        return Vec::new();
    }

    // Cooldown: one alert per burst, not one per error over the threshold.
    let last_alert = LAST_ERROR_ALERT.get_or_init(|| StdMutex::new(None));
    {
        let mut last = last_alert.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(prev) = *last {
            if now - prev < chrono::Duration::seconds(config.diagnostic_cooldown_secs.max(1)) {
                return Vec::new();
            }
        }
        *last = Some(now);
    }

    vec![AnomalyFinding {
        kind: AnomalyKind::DiagnosticErrorBurst,
        severity: AnomalySeverity::High,
        reason: format!(
            "{count} error-level diagnostics in {}s (latest component: {component})",
            config.window_secs
        ),
        source: component.to_string(),
        host: "local".to_string(),
        command: message.chars().take(200).collect(),
        count,
        threshold: config.diagnostic_error_threshold,
        window_secs: config.window_secs,
    }]
}

pub fn publish_anomalies(findings: &[AnomalyFinding]) {
    for finding in findings {
        publish_event(
            EventType::AnomalyDetected,
            serde_json::to_value(finding).unwrap_or_default(),
        );
        fire_anomaly_webhook(finding.clone());
    }
}

#[cfg(feature = "daemon")]
fn fire_anomaly_webhook(finding: AnomalyFinding) {
    // The diagnostic error sink can fire this from non-runtime threads (e.g. an
    // embedded-SSH worker thread logging an error), so guard the spawn instead of
    // using `tokio::spawn`, which would panic outside a runtime.
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    handle.spawn(async move {
        let event = WebhookEvent {
            event: "anomaly_detected".into(),
            host: finding.host,
            command: finding.command,
            approval_id: None,
            risk_level: Some(format!("{:?}", finding.severity).to_lowercase()),
            exit_code: None,
        };
        if let Err(e) = crate::notify::fire_webhook(event).await {
            tracing::error!(error = %e, "anomaly webhook error");
        }
    });
}

#[cfg(feature = "daemon")]
use crate::notify::WebhookEvent;

#[cfg(not(feature = "daemon"))]
fn fire_anomaly_webhook(_finding: AnomalyFinding) {}

fn anomaly_config_path() -> Result<PathBuf> {
    Ok(crate::store::config_dir()?.join("anomaly.toml"))
}

fn dedupe_findings(findings: Vec<AnomalyFinding>) -> Vec<AnomalyFinding> {
    let mut seen = HashMap::new();
    for finding in findings {
        seen.entry(format!("{:?}", finding.kind)).or_insert(finding);
    }
    seen.into_values().collect()
}

fn command_matches(command: &str, pattern: &str) -> bool {
    let pattern = pattern.trim().to_lowercase();
    if pattern.is_empty() {
        return false;
    }
    if pattern.contains('*') {
        let mut pos = 0;
        for part in pattern.split('*').filter(|part| !part.is_empty()) {
            match command[pos..].find(part) {
                Some(idx) => pos += idx + part.len(),
                None => return false,
            }
        }
        true
    } else {
        command.contains(&pattern)
    }
}

fn is_after_hours(hour: u32, start: u32, end: u32) -> bool {
    let start = start.min(23);
    let end = end.min(23);
    if start == end {
        return false;
    }
    if start < end {
        hour >= start && hour < end
    } else {
        hour >= start || hour < end
    }
}

fn default_true() -> bool {
    true
}
fn default_window_secs() -> i64 {
    300
}
fn default_source_burst_threshold() -> usize {
    10
}
fn default_sensitive_threshold() -> usize {
    1
}
fn default_sensitive_patterns() -> Vec<String> {
    vec![
        "sudo*".into(),
        "rm -rf*".into(),
        "terraform destroy*".into(),
        "kubectl delete*".into(),
        "chmod 777*".into(),
        "iptables*".into(),
        "drop table*".into(),
    ]
}
fn default_diagnostic_error_threshold() -> usize {
    5
}
fn default_diagnostic_cooldown_secs() -> i64 {
    120
}
fn default_after_hours_start() -> u32 {
    22
}
fn default_after_hours_end() -> u32 {
    6
}
fn default_after_hours_risks() -> Vec<RiskLevel> {
    vec![RiskLevel::High, RiskLevel::Blocked]
}

trait DateTimeHour {
    fn hour(&self) -> u32;
}

impl DateTimeHour for chrono::DateTime<chrono::Utc> {
    fn hour(&self) -> u32 {
        chrono::Timelike::hour(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn audit(source: &str, command: &str, risk_level: RiskLevel, offset_secs: i64) -> AuditEntry {
        AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now() + chrono::Duration::seconds(offset_secs),
            host: "web".into(),
            command: command.into(),
            exit_code: Some(0),
            duration_ms: 10,
            risk_level,
            reason: None,
            change_id: None,
            source: Some(source.into()),
        }
    }

    #[test]
    fn detects_source_burst() {
        let config = AnomalyConfig {
            source_burst_threshold: 3,
            sensitive_threshold: 0,
            after_hours_risks: vec![],
            ..Default::default()
        };
        let entries = vec![
            audit("mcp", "ls", RiskLevel::Low, -2),
            audit("mcp", "pwd", RiskLevel::Low, -1),
            audit("mcp", "whoami", RiskLevel::Low, 0),
        ];
        let findings = detect_anomalies(&entries, entries.last().unwrap(), &config);
        assert!(findings.iter().any(|f| f.kind == AnomalyKind::SourceBurst));
    }

    #[test]
    fn detects_sensitive_pattern() {
        let config = AnomalyConfig {
            source_burst_threshold: 0,
            after_hours_risks: vec![],
            ..Default::default()
        };
        let current = audit("cli", "terraform destroy -auto-approve", RiskLevel::High, 0);
        let findings = detect_anomalies(std::slice::from_ref(&current), &current, &config);
        assert!(findings
            .iter()
            .any(|f| f.kind == AnomalyKind::SensitivePattern));
    }

    #[test]
    fn after_hours_wraps_midnight() {
        assert!(is_after_hours(23, 22, 6));
        assert!(is_after_hours(3, 22, 6));
        assert!(!is_after_hours(12, 22, 6));
    }

    #[test]
    #[serial_test::serial]
    fn diagnostic_error_burst_trips_then_cools_down() {
        // Isolate config (defaults: threshold 5) under a temp dir.
        let config_dir = std::env::temp_dir().join(format!("agent2ssh-anomaly-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&config_dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &config_dir);

        // Reset the shared sliding-window state so prior tests don't bleed in.
        if let Some(times) = ERROR_TIMES.get() {
            times.lock().unwrap().clear();
        }
        if let Some(last) = LAST_ERROR_ALERT.get() {
            *last.lock().unwrap() = None;
        }

        // First four errors stay under the default threshold of 5.
        for _ in 0..4 {
            assert!(record_diagnostic_error("test", "boom").is_empty());
        }
        // The fifth crosses the threshold and yields exactly one finding.
        let tripped = record_diagnostic_error("test", "boom");
        assert_eq!(tripped.len(), 1);
        assert_eq!(tripped[0].kind, AnomalyKind::DiagnosticErrorBurst);

        // Cooldown suppresses immediate repeat alerts.
        assert!(record_diagnostic_error("test", "boom").is_empty());

        // Webhook-delivery errors are ignored so they cannot self-trigger.
        assert!(record_diagnostic_error("daemon", "anomaly webhook error").is_empty());

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&config_dir);
    }
}
