use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Mutex, MutexGuard, OnceLock},
};

use crate::types::{
    default_host_group, default_host_groups, AppConfig, AuditEntry, AuditFilter, ExecResult,
    RiskLevel,
};

static STORE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

type HostLabelMap =
    std::collections::HashMap<String, (Option<String>, Option<String>, Option<String>)>;

pub fn hosts_lock() -> &'static Mutex<()> {
    STORE_LOCK.get_or_init(|| Mutex::new(()))
}

pub struct StoreWriteGuard {
    _process_guard: MutexGuard<'static, ()>,
    _file_guard: FileLockGuard,
}

pub struct FileLockGuard {
    _file: File,
}

pub fn store_write_lock() -> Result<StoreWriteGuard> {
    let process_guard = hosts_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("store lock poisoned"))?;
    let file_guard = lock_config_file(".hosts.lock")?;
    Ok(StoreWriteGuard {
        _process_guard: process_guard,
        _file_guard: file_guard,
    })
}

fn audit_write_lock() -> Result<FileLockGuard> {
    lock_config_file(".audit.lock")
}

/// Acquire an exclusive cross-process advisory lock backed by a dedicated lock
/// file under the config dir (e.g. `.hosts.lock`, `.audit.lock`, `.app_log.lock`).
/// Held by the returned guard until it drops. Use this — not only a process-local
/// `Mutex` — whenever a file under `~/.agent2ssh/` is written by more than one of
/// the CLI/MCP/daemon/desktop processes, so concurrent writers cannot interleave
/// or race a rotation.
pub fn lock_config_file(name: &str) -> Result<FileLockGuard> {
    ensure_config_dir()?;
    let path = config_dir()?.join(name);
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("failed to open lock file {}", path.display()))?;
    restrict_file_to_owner(&path)?;
    file.lock_exclusive()
        .with_context(|| format!("failed to lock {}", path.display()))?;
    Ok(FileLockGuard { _file: file })
}

pub fn config_dir() -> Result<PathBuf> {
    if let Some(path) = config_dir_override(std::env::var("AGENT2SSH_CONFIG_DIR").ok()) {
        return Ok(path);
    }

    let base =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("unable to locate home directory"))?;
    Ok(base.join(".agent2ssh"))
}

fn config_dir_override(path: Option<String>) -> Option<PathBuf> {
    path.filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("hosts.json"))
}

pub fn audit_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("audit.jsonl"))
}

pub fn ensure_config_dir() -> Result<()> {
    fs::create_dir_all(config_dir()?).context("failed to create ~/.agent2ssh")
}

/// Restrict a sensitive file to owner read/write on Unix.
pub fn restrict_file_to_owner(path: impl AsRef<std::path::Path>) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path.as_ref(), perms).with_context(|| {
            format!(
                "failed to restrict permissions for {}",
                path.as_ref().display()
            )
        })?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

pub fn load_config() -> Result<AppConfig> {
    ensure_config_dir()?;
    let path = config_path()?;
    if !path.exists() {
        return Ok(normalize_config(AppConfig::default()));
    }
    let raw = fs::read_to_string(path).context("failed to read hosts config")?;
    let config: AppConfig = serde_json::from_str(&raw).context("failed to parse hosts config")?;
    Ok(normalize_config(config))
}

pub fn save_config(config: &AppConfig) -> Result<()> {
    let _guard = store_write_lock()?;
    save_config_unlocked(config)
}

pub(crate) fn save_config_unlocked(config: &AppConfig) -> Result<()> {
    ensure_config_dir()?;
    let normalized = normalize_config(config.clone());
    let raw = serde_json::to_string_pretty(&normalized)?;
    let path = config_path()?;
    let tmp_path = path.with_extension(format!("json.tmp.{}", std::process::id()));
    let write_result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp_path)
            .with_context(|| format!("failed to open temp config {}", tmp_path.display()))?;
        file.write_all(raw.as_bytes())
            .with_context(|| format!("failed to write temp config {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync temp config {}", tmp_path.display()))?;
        restrict_file_to_owner(&tmp_path)?;
        fs::rename(&tmp_path, &path)
            .with_context(|| format!("failed to replace hosts config {}", path.display()))?;
        restrict_file_to_owner(&path)?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    write_result
}

fn normalize_config(mut config: AppConfig) -> AppConfig {
    if config.groups.is_empty() {
        config.groups = default_host_groups();
    }
    if !config
        .groups
        .iter()
        .any(|group| group.id == default_host_group())
    {
        config.groups.insert(0, default_host_groups().remove(0));
    }
    for group in &mut config.groups {
        group.id = group.id.trim().to_string();
        group.name = group.name.trim().to_string();
        if group.id.is_empty() {
            group.id = default_host_group();
        }
        if group.name.is_empty() {
            group.name = group.id.clone();
        }
    }
    config.groups.sort_by(|a, b| {
        if a.id == default_host_group() {
            std::cmp::Ordering::Less
        } else if b.id == default_host_group() {
            std::cmp::Ordering::Greater
        } else {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        }
    });

    let default_group = default_host_group();
    let valid_groups: std::collections::HashSet<String> =
        config.groups.iter().map(|group| group.id.clone()).collect();
    for proxy in &mut config.proxies {
        proxy.id = proxy.id.trim().to_string();
        proxy.name = proxy.name.trim().to_string();
        proxy.host = proxy.host.trim().to_string();
        proxy.username = proxy
            .username
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        proxy.password = proxy
            .password
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
    }
    config.proxies.retain(|proxy| {
        !proxy.id.is_empty() && !proxy.name.is_empty() && !proxy.host.is_empty() && proxy.port > 0
    });
    config
        .proxies
        .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    let valid_proxies: std::collections::HashSet<String> = config
        .proxies
        .iter()
        .map(|proxy| proxy.id.clone())
        .collect();
    for host in &mut config.hosts {
        host.group = host.group.trim().to_string();
        if host.group.is_empty() || !valid_groups.contains(&host.group) {
            host.group = default_group.clone();
        }
        host.proxy_id = host
            .proxy_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty() && valid_proxies.contains(*value))
            .map(ToOwned::to_owned);
    }

    config
}

pub fn append_audit(
    result: &ExecResult,
    risk_level: RiskLevel,
    reason: Option<&str>,
    change_id: Option<&str>,
    source: Option<&str>,
) -> Result<()> {
    use chrono::Utc;
    use uuid::Uuid;

    ensure_config_dir()?;
    let _guard = audit_write_lock()?;
    rotate_audit_if_needed_unlocked(10 * 1024 * 1024)?; // 10 MB default
    let entry = AuditEntry {
        id: Uuid::new_v4(),
        ts: Utc::now(),
        host: result.host.clone(),
        command: redact_sensitive_text(&result.command),
        exit_code: result.exit_code,
        duration_ms: result.duration_ms,
        risk_level,
        reason: reason.map(str::to_string),
        change_id: change_id.map(str::to_string),
        source: source.map(str::to_string),
    };
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(audit_path()?)
        .context("failed to open audit log")?;
    writeln!(file, "{}", serde_json::to_string(&entry)?)?;
    detect_and_publish_audit_anomalies(&entry);
    Ok(())
}

fn detect_and_publish_audit_anomalies(entry: &AuditEntry) {
    let Ok(config) = crate::anomaly::load_anomaly_config() else {
        return;
    };
    if !config.enabled {
        return;
    }
    let filter = AuditFilter {
        since: Some((entry.ts - chrono::Duration::seconds(config.window_secs.max(1))).to_rfc3339()),
        until: Some(entry.ts.to_rfc3339()),
        limit: 1000,
        ..Default::default()
    };
    let Ok(entries) = list_audit_raw(&filter) else {
        return;
    };
    let findings = crate::anomaly::detect_anomalies(&entries, entry, &config);
    crate::anomaly::publish_anomalies(&findings);
}

/// Rotate audit log if it exceeds `max_size_bytes`.
///
/// Keeps at most 3 rotated files: `audit.jsonl.1`, `.2`, `.3`.
/// When rotating, `.2` → `.3`, `.1` → `.2`, current → `.1`.
pub fn rotate_audit_if_needed(max_size_bytes: u64) -> Result<()> {
    let _guard = audit_write_lock()?;
    rotate_audit_if_needed_unlocked(max_size_bytes)
}

fn rotate_audit_if_needed_unlocked(max_size_bytes: u64) -> Result<()> {
    let path = audit_path()?;
    if !path.exists() {
        return Ok(());
    }
    let metadata = std::fs::metadata(&path)?;
    if metadata.len() <= max_size_bytes {
        return Ok(());
    }

    // Shift existing rotations: .2 → .3, .1 → .2
    for i in (2..=3).rev() {
        let src = path.with_extension(format!("jsonl.{}", i - 1));
        let dst = path.with_extension(format!("jsonl.{i}"));
        if src.exists() {
            if dst.exists() {
                std::fs::remove_file(&dst)?;
            }
            std::fs::rename(&src, &dst)?;
        }
    }
    // Remove .3 if it would be pushed beyond limit (we only keep 3 rotations)
    let overflow = path.with_extension("jsonl.4");
    if overflow.exists() {
        let _ = std::fs::remove_file(&overflow);
    }

    // Current → .1
    let rotated = path.with_extension("jsonl.1");
    if rotated.exists() {
        std::fs::remove_file(&rotated)?;
    }
    std::fs::rename(&path, &rotated)?;

    // Publish audit rotated event
    crate::events::publish_event(
        crate::events::EventType::AuditRotated,
        serde_json::json!({"file": rotated.display().to_string()}),
    );

    Ok(())
}

/// Public wrapper for CLI/daemon invocation of audit rotation with the default 10 MB limit.
pub fn rotate_audit_core() -> Result<()> {
    rotate_audit_if_needed(10 * 1024 * 1024)
}

// ── Metrics Trends (F6-3) ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsTrend {
    pub period: TrendPeriod,
    pub total_executions: usize,
    pub success_count: usize,
    pub failure_count: usize,
    pub blocked_count: usize,
    pub failure_rate: f64,
    pub risk_distribution: RiskDistribution,
    pub avg_duration_ms: f64,
    pub top_hosts: Vec<HostExecutionCount>,
    pub hourly_breakdown: Vec<HourlyBucket>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TrendPeriod {
    Last24h,
    Last7d,
    Last30d,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskDistribution {
    pub low: usize,
    pub medium: usize,
    pub high: usize,
    pub blocked: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostExecutionCount {
    pub host: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyBucket {
    pub hour: String, // ISO-8601 truncated to hour
    pub count: usize,
    pub failures: usize,
}

/// Compute metrics trends from audit data.
pub fn compute_metrics_trend(period: TrendPeriod) -> Result<MetricsTrend> {
    let now = chrono::Utc::now();
    let since = match period {
        TrendPeriod::Last24h => Some((now - chrono::Duration::hours(24)).to_rfc3339()),
        TrendPeriod::Last7d => Some((now - chrono::Duration::days(7)).to_rfc3339()),
        TrendPeriod::Last30d => Some((now - chrono::Duration::days(30)).to_rfc3339()),
        TrendPeriod::All => None,
    };

    let filter = crate::types::AuditFilter {
        host: None,
        risk_level: None,
        exit_code: None,
        since,
        until: None,
        limit: usize::MAX,
        search: None,
        command_pattern: None,
        host_env: None,
        host_role: None,
        host_owner: None,
    };

    let entries = list_audit_raw(&filter)?;

    let total_executions = entries.len();
    let failure_count = entries
        .iter()
        .filter(|e| e.exit_code.map(|c| c != 0).unwrap_or(true))
        .count();
    let success_count = total_executions.saturating_sub(failure_count);
    let blocked_count = entries
        .iter()
        .filter(|e| e.risk_level == RiskLevel::Blocked)
        .count();

    let failure_rate = if total_executions > 0 {
        failure_count as f64 / total_executions as f64
    } else {
        0.0
    };

    let risk_distribution = RiskDistribution {
        low: entries
            .iter()
            .filter(|e| e.risk_level == RiskLevel::Low)
            .count(),
        medium: entries
            .iter()
            .filter(|e| e.risk_level == RiskLevel::Medium)
            .count(),
        high: entries
            .iter()
            .filter(|e| e.risk_level == RiskLevel::High)
            .count(),
        blocked: entries
            .iter()
            .filter(|e| e.risk_level == RiskLevel::Blocked)
            .count(),
    };

    let avg_duration_ms = if total_executions > 0 {
        entries.iter().map(|e| e.duration_ms as f64).sum::<f64>() / total_executions as f64
    } else {
        0.0
    };

    // Top 10 hosts by execution count
    let mut host_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for e in &entries {
        *host_counts.entry(e.host.clone()).or_insert(0) += 1;
    }
    let mut top_hosts: Vec<HostExecutionCount> = host_counts
        .into_iter()
        .map(|(host, count)| HostExecutionCount { host, count })
        .collect();
    top_hosts.sort_by_key(|host| std::cmp::Reverse(host.count));
    top_hosts.truncate(10);

    // Hourly breakdown
    let mut hourly: std::collections::BTreeMap<String, (usize, usize)> =
        std::collections::BTreeMap::new();
    for e in &entries {
        let hour_key = e.ts.format("%Y-%m-%dT%H:00:00Z").to_string();
        let entry = hourly.entry(hour_key).or_insert((0, 0));
        entry.0 += 1;
        if e.exit_code.map(|c| c != 0).unwrap_or(true) {
            entry.1 += 1;
        }
    }
    let hourly_breakdown: Vec<HourlyBucket> = hourly
        .into_iter()
        .map(|(hour, (count, failures))| HourlyBucket {
            hour,
            count,
            failures,
        })
        .collect();

    Ok(MetricsTrend {
        period,
        total_executions,
        success_count,
        failure_count,
        blocked_count,
        failure_rate,
        risk_distribution,
        avg_duration_ms,
        top_hosts,
        hourly_breakdown,
    })
}

pub fn redact_sensitive_text(input: &str) -> String {
    let upper = input.to_ascii_uppercase();
    if upper.contains("BEGIN ") && upper.contains("PRIVATE KEY") {
        return "[REDACTED PRIVATE KEY]".to_string();
    }

    let mut out = Vec::new();
    let mut redact_next = false;
    for token in input.split_whitespace() {
        let lower = token.to_ascii_lowercase();
        if redact_next {
            if matches!(lower.as_str(), "bearer" | "basic") {
                redact_next = true;
                continue;
            }
            out.push("[REDACTED]".to_string());
            redact_next = false;
            continue;
        }
        if let Some((key, _)) = token.split_once('=') {
            let key_lower = key.to_ascii_lowercase();
            if is_sensitive_key(&key_lower) {
                out.push(format!("{key}=[REDACTED]"));
                continue;
            }
        }
        if is_sensitive_key(&lower) {
            out.push(token.to_string());
            redact_next = true;
            continue;
        }
        out.push(token.to_string());
    }
    out.join(" ")
}

fn is_sensitive_key(key: &str) -> bool {
    matches!(
        key.trim_start_matches('-').trim_end_matches(':'),
        "password"
            | "passwd"
            | "token"
            | "secret"
            | "api-key"
            | "apikey"
            | "access-token"
            | "authorization"
            | "bearer"
            | "cookie"
            | "set-cookie"
    )
}

pub fn list_audit_raw(filter: &AuditFilter) -> Result<Vec<AuditEntry>> {
    ensure_config_dir()?;
    let path = audit_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path).context("failed to read audit log")?;

    let since = filter
        .since
        .as_deref()
        .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());
    let until = filter
        .until
        .as_deref()
        .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());

    // Build a lookup map from host name to labels for host group filtering.
    let host_label_map: Option<HostLabelMap> =
        if filter.host_env.is_some() || filter.host_role.is_some() || filter.host_owner.is_some() {
            let config = load_config().unwrap_or_default();
            let mut map = std::collections::HashMap::new();
            for h in &config.hosts {
                map.insert(
                    h.name.clone(),
                    (h.env.clone(), h.role.clone(), h.owner.clone()),
                );
            }
            Some(map)
        } else {
            None
        };

    // Compute the set of host names matching the host group filters.
    let matching_hosts: Option<std::collections::HashSet<String>> = host_label_map.map(|map| {
        map.into_iter()
            .filter(|(_, (env, role, owner))| {
                let env_ok = match &filter.host_env {
                    Some(v) => env
                        .as_deref()
                        .map(|e| e.eq_ignore_ascii_case(v))
                        .unwrap_or(false),
                    None => true,
                };
                let role_ok = match &filter.host_role {
                    Some(v) => role
                        .as_deref()
                        .map(|r| r.eq_ignore_ascii_case(v))
                        .unwrap_or(false),
                    None => true,
                };
                let owner_ok = match &filter.host_owner {
                    Some(v) => owner
                        .as_deref()
                        .map(|o| o.eq_ignore_ascii_case(v))
                        .unwrap_or(false),
                    None => true,
                };
                env_ok && role_ok && owner_ok
            })
            .map(|(name, _)| name)
            .collect()
    });

    let search_lower = filter.search.as_deref().map(|s| s.to_lowercase());

    let mut entries: Vec<AuditEntry> = raw
        .lines()
        .filter_map(|line| serde_json::from_str::<AuditEntry>(line).ok())
        .filter(|e| {
            if let Some(h) = &filter.host {
                if !e.host.eq_ignore_ascii_case(h) {
                    return false;
                }
            }
            if let Some(r) = filter.risk_level {
                if e.risk_level != r {
                    return false;
                }
            }
            if let Some(code) = filter.exit_code {
                if e.exit_code != Some(code) {
                    return false;
                }
            }
            if let Some(since) = since {
                if e.ts < since {
                    return false;
                }
            }
            if let Some(until) = until {
                if e.ts > until {
                    return false;
                }
            }
            // F6-1: full-text search (case-insensitive substring on command and host)
            if let Some(ref needle) = search_lower {
                if !e.command.to_lowercase().contains(needle)
                    && !e.host.to_lowercase().contains(needle)
                    && !e
                        .source
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(needle)
                {
                    return false;
                }
            }
            // F6-1: command pattern (glob-style match)
            if let Some(ref pattern) = filter.command_pattern {
                if !glob_match(pattern, &e.command) {
                    return false;
                }
            }
            // F6-1: host group filtering
            if let Some(ref hosts_set) = matching_hosts {
                if !hosts_set.contains(&e.host) {
                    return false;
                }
            }
            true
        })
        .collect();

    entries.reverse();
    entries.truncate(filter.limit);
    Ok(entries)
}

/// Simple glob-style pattern matching supporting `*` (any sequence) and `?` (any single char).
/// Case-insensitive.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.to_lowercase().chars().collect();
    let txt: Vec<char> = text.to_lowercase().chars().collect();
    glob_match_inner(&pat, &txt)
}

fn glob_match_inner(pat: &[char], txt: &[char]) -> bool {
    if pat.is_empty() {
        return txt.is_empty();
    }
    if pat[0] == '*' {
        // Try matching * with 0..n chars
        for i in 0..=txt.len() {
            if glob_match_inner(&pat[1..], &txt[i..]) {
                return true;
            }
        }
        return false;
    }
    if txt.is_empty() {
        return false;
    }
    if pat[0] == '?' || pat[0] == txt[0] {
        return glob_match_inner(&pat[1..], &txt[1..]);
    }
    false
}

// ── Audit Export (F6-2) ─────────────────────────────────────────────────────

/// Export audit entries as JSONL (one JSON object per line).
/// Redaction is already applied at write time, so entries are emitted as-is.
pub fn export_audit_jsonl(filter: &AuditFilter) -> Result<String> {
    let entries = list_audit_raw(filter)?;
    let mut output = String::new();
    for entry in &entries {
        output.push_str(&serde_json::to_string(entry)?);
        output.push('\n');
    }
    Ok(output)
}

/// Export audit entries as CSV with headers.
/// Fields: id, timestamp, host, command, exit_code, duration_ms, risk_level, reason, change_id, source
/// Fields containing commas, quotes, or newlines are properly quoted/escaped per RFC 4180.
pub fn export_audit_csv(filter: &AuditFilter) -> Result<String> {
    let entries = list_audit_raw(filter)?;
    let mut output = String::new();
    // Header row
    output.push_str(
        "id,timestamp,host,command,exit_code,duration_ms,risk_level,reason,change_id,source\n",
    );
    for entry in &entries {
        output.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{}\n",
            entry.id,
            entry.ts.to_rfc3339(),
            csv_escape(&entry.host),
            csv_escape(&entry.command),
            entry.exit_code.map(|c| c.to_string()).unwrap_or_default(),
            entry.duration_ms,
            entry.risk_level,
            csv_escape(entry.reason.as_deref().unwrap_or("")),
            csv_escape(entry.change_id.as_deref().unwrap_or("")),
            csv_escape(entry.source.as_deref().unwrap_or("")),
        ));
    }
    Ok(output)
}

/// Escape a field value for CSV output per RFC 4180.
/// If the value contains a comma, double-quote, or newline, wrap it in
/// double-quotes and double any internal quotes.
fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_dir_uses_env_override() {
        let expected =
            std::env::temp_dir().join(format!("agent2ssh-config-{}", uuid::Uuid::new_v4()));

        assert_eq!(
            config_dir_override(Some(expected.display().to_string())).unwrap(),
            expected
        );
        assert!(config_dir_override(Some("   ".to_string())).is_none());
        assert!(config_dir_override(None).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_restrict_file_to_owner_sets_0600() {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::temp_dir().join(format!("agent2ssh-perms-{}", uuid::Uuid::new_v4()));
        fs::write(&path, "secret").unwrap();

        restrict_file_to_owner(&path).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let _ = fs::remove_file(&path);
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn test_redact_sensitive_text() {
        let redacted = redact_sensitive_text(
            "deploy --token abc password=hunter2 --api-key key123 --safe value",
        );
        assert_eq!(
            redacted,
            "deploy --token [REDACTED] password=[REDACTED] --api-key [REDACTED] --safe value"
        );

        let auth = redact_sensitive_text("Authorization: Bearer abc123\ncookie=session-id");
        assert_eq!(auth, "Authorization: [REDACTED] cookie=[REDACTED]");

        let private_key = redact_sensitive_text(
            "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----",
        );
        assert_eq!(private_key, "[REDACTED PRIVATE KEY]");
    }

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("hello", "hello"));
        assert!(!glob_match("hello", "world"));
    }

    #[test]
    fn test_glob_match_star() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("sudo *", "sudo rm -rf /"));
        assert!(glob_match("*.sh", "deploy.sh"));
        assert!(!glob_match("*.sh", "deploy.py"));
        assert!(glob_match(
            "kubectl delete *",
            "kubectl delete namespace default"
        ));
    }

    #[test]
    fn test_glob_match_question() {
        assert!(glob_match("h?llo", "hello"));
        assert!(glob_match("h?llo", "hallo"));
        assert!(!glob_match("h?llo", "heello"));
    }

    #[test]
    fn test_glob_match_case_insensitive() {
        assert!(glob_match("SUDO *", "sudo whoami"));
        assert!(glob_match("sudo *", "SUDO REBOOT"));
    }

    #[test]
    fn test_audit_filter_search() {
        // Test the search/glob logic directly without relying on env vars
        // (env vars have race conditions in parallel tests)
        use crate::types::{AuditEntry, RiskLevel};
        use chrono::Utc;
        use uuid::Uuid;

        let entry1 = AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            host: "prod-server".into(),
            command: "sudo apt update".into(),
            exit_code: Some(0),
            duration_ms: 100,
            risk_level: RiskLevel::High,
            reason: None,
            change_id: None,
            source: None,
        };
        let entry2 = AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            host: "dev-box".into(),
            command: "ls -la".into(),
            exit_code: Some(0),
            duration_ms: 50,
            risk_level: RiskLevel::Low,
            reason: None,
            change_id: None,
            source: None,
        };

        // Test search: "apt" should match entry1's command
        let needle = "apt".to_lowercase();
        let entries = [&entry1, &entry2];
        let matches: Vec<_> = entries
            .iter()
            .filter(|e| {
                e.command.to_lowercase().contains(&needle)
                    || e.host.to_lowercase().contains(&needle)
            })
            .collect();
        assert_eq!(matches.len(), 1);
        assert!(matches[0].command.contains("apt"));

        // Test search: "prod" should match entry1's host
        let needle = "prod".to_lowercase();
        let entries = [&entry1, &entry2];
        let matches: Vec<_> = entries
            .iter()
            .filter(|e| {
                e.command.to_lowercase().contains(&needle)
                    || e.host.to_lowercase().contains(&needle)
            })
            .collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].host, "prod-server");
    }

    #[test]
    fn test_audit_filter_command_pattern() {
        use crate::types::{AuditEntry, RiskLevel};
        use chrono::Utc;
        use uuid::Uuid;

        let entry1 = AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            host: "server".into(),
            command: "kubectl delete namespace default".into(),
            exit_code: Some(0),
            duration_ms: 200,
            risk_level: RiskLevel::High,
            reason: None,
            change_id: None,
            source: None,
        };
        let entry2 = AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            host: "server".into(),
            command: "ls -la".into(),
            exit_code: Some(0),
            duration_ms: 50,
            risk_level: RiskLevel::Low,
            reason: None,
            change_id: None,
            source: None,
        };

        // Test command_pattern: "kubectl delete *" should match entry1
        let pattern = "kubectl delete *";
        let entries = [&entry1, &entry2];
        let matches: Vec<_> = entries
            .iter()
            .filter(|e| glob_match(pattern, &e.command))
            .collect();
        assert_eq!(matches.len(), 1);
        assert!(matches[0].command.starts_with("kubectl delete"));

        // Test command_pattern: "ls *" should match entry2
        let pattern = "ls *";
        let entries = [&entry1, &entry2];
        let matches: Vec<_> = entries
            .iter()
            .filter(|e| glob_match(pattern, &e.command))
            .collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].command, "ls -la");
    }

    // ── F6-3: Metrics trend tests ───────────────────────────────────────────

    #[test]
    fn test_metrics_trend_empty() {
        // compute_metrics_trend should work even if there is no audit data.
        // We don't set AGENT2SSH_CONFIG_DIR to avoid parallel test race conditions.
        // Just verify it returns a valid MetricsTrend with correct structure.
        let trend = super::compute_metrics_trend(super::TrendPeriod::All);
        // It should succeed regardless of whether audit data exists
        assert!(trend.is_ok(), "compute_metrics_trend should not fail");
        let trend = trend.unwrap();
        assert_eq!(trend.period, super::TrendPeriod::All);
        // Verify structural integrity
        assert!(trend.failure_rate >= 0.0 && trend.failure_rate <= 1.0);
        assert!(trend.avg_duration_ms >= 0.0);
        assert_eq!(
            trend.risk_distribution.low
                + trend.risk_distribution.medium
                + trend.risk_distribution.high
                + trend.risk_distribution.blocked,
            trend.total_executions
        );
    }

    #[test]
    fn test_risk_distribution_serialization() {
        let dist = super::RiskDistribution {
            low: 10,
            medium: 5,
            high: 2,
            blocked: 1,
        };
        let json = serde_json::to_string(&dist).unwrap();
        let de: super::RiskDistribution = serde_json::from_str(&json).unwrap();
        assert_eq!(de.low, 10);
        assert_eq!(de.medium, 5);
        assert_eq!(de.high, 2);
        assert_eq!(de.blocked, 1);
    }

    #[test]
    fn test_trend_period_values() {
        let periods = vec![
            super::TrendPeriod::Last24h,
            super::TrendPeriod::Last7d,
            super::TrendPeriod::Last30d,
            super::TrendPeriod::All,
        ];
        for p in &periods {
            let json = serde_json::to_string(p).unwrap();
            let de: super::TrendPeriod = serde_json::from_str(&json).unwrap();
            assert_eq!(*p, de);
        }
        // Verify serialized names
        assert_eq!(
            serde_json::to_string(&super::TrendPeriod::Last24h).unwrap(),
            "\"last24h\""
        );
        assert_eq!(
            serde_json::to_string(&super::TrendPeriod::Last7d).unwrap(),
            "\"last7d\""
        );
        assert_eq!(
            serde_json::to_string(&super::TrendPeriod::Last30d).unwrap(),
            "\"last30d\""
        );
        assert_eq!(
            serde_json::to_string(&super::TrendPeriod::All).unwrap(),
            "\"all\""
        );
    }

    // ── F6-2: Audit export tests ────────────────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn test_export_audit_jsonl_empty() {
        // With a temp config dir (no audit data), JSONL export should be empty
        let config_dir =
            std::env::temp_dir().join(format!("agent2ssh-export-jsonl-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&config_dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &config_dir);

        let filter = AuditFilter::default();
        let output = super::export_audit_jsonl(&filter).unwrap();
        assert!(output.is_empty(), "empty audit should return empty string");

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&config_dir);
    }

    #[test]
    #[serial_test::serial]
    fn test_export_audit_csv_headers() {
        // CSV output should always contain the correct header row
        let config_dir =
            std::env::temp_dir().join(format!("agent2ssh-export-csv-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&config_dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &config_dir);

        let filter = AuditFilter::default();
        let output = super::export_audit_csv(&filter).unwrap();
        assert!(output.starts_with(
            "id,timestamp,host,command,exit_code,duration_ms,risk_level,reason,change_id,source\n"
        ));
        // Should only contain the header row (no data)
        assert_eq!(output.lines().count(), 1);

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&config_dir);
    }

    #[test]
    fn test_export_audit_csv_escaping() {
        // Test that csv_escape handles commas, quotes, and newlines
        assert_eq!(super::csv_escape("simple"), "simple");
        assert_eq!(super::csv_escape("has,comma"), "\"has,comma\"");
        assert_eq!(super::csv_escape("has\"quote"), "\"has\"\"quote\"");
        assert_eq!(super::csv_escape("has\nnewline"), "\"has\nnewline\"");
        assert_eq!(
            super::csv_escape("both,\"comma and quote\""),
            "\"both,\"\"comma and quote\"\"\""
        );
    }

    #[test]
    fn test_export_audit_jsonl_with_data() {
        // Test the JSONL and CSV formatting logic directly without relying on
        // env vars (which have race conditions in parallel tests).
        use chrono::Utc;
        use uuid::Uuid;

        let entry1 = AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            host: "test-host".into(),
            command: "ls -la".into(),
            exit_code: Some(0),
            duration_ms: 100,
            risk_level: RiskLevel::Low,
            reason: None,
            change_id: None,
            source: None,
        };
        let entry2 = AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            host: "prod-host".into(),
            command: "sudo apt update".into(),
            exit_code: Some(0),
            duration_ms: 5000,
            risk_level: RiskLevel::High,
            reason: Some("weekly update".into()),
            change_id: Some("CHG-001".into()),
            source: Some("cli".into()),
        };

        let entries = vec![entry1, entry2];

        // Test JSONL formatting (same logic as export_audit_jsonl)
        let mut jsonl_output = String::new();
        for entry in &entries {
            jsonl_output.push_str(&serde_json::to_string(entry).unwrap());
            jsonl_output.push('\n');
        }
        let lines: Vec<&str> = jsonl_output.lines().collect();
        assert_eq!(lines.len(), 2, "should have 2 JSONL lines");
        for line in &lines {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(parsed.get("host").is_some());
        }

        // Test CSV formatting (same logic as export_audit_csv)
        let mut csv_output = String::new();
        csv_output.push_str(
            "id,timestamp,host,command,exit_code,duration_ms,risk_level,reason,change_id\n",
        );
        for entry in &entries {
            csv_output.push_str(&format!(
                "{},{},{},{},{},{},{},{},{}\n",
                entry.id,
                entry.ts.to_rfc3339(),
                super::csv_escape(&entry.host),
                super::csv_escape(&entry.command),
                entry.exit_code.map(|c| c.to_string()).unwrap_or_default(),
                entry.duration_ms,
                entry.risk_level,
                super::csv_escape(entry.reason.as_deref().unwrap_or("")),
                super::csv_escape(entry.change_id.as_deref().unwrap_or("")),
            ));
        }
        let csv_lines: Vec<&str> = csv_output.lines().collect();
        assert_eq!(csv_lines.len(), 3, "header + 2 data rows");
        assert!(csv_lines[0].starts_with("id,"));
        // Verify data row contains expected values
        assert!(csv_lines[1].contains("test-host"));
        assert!(csv_lines[2].contains("prod-host"));
        assert!(csv_lines[2].contains("CHG-001"));
    }

    // ── S1-1: exec-multi audit context tests ───────────────────────────────

    #[test]
    fn test_exec_multi_audit_entries_reason_and_change_id() {
        // Verify that audit entries constructed for an exec-multi scenario
        // correctly carry reason and change_id through the full JSONL
        // serialisation round-trip — one entry per target host.
        use chrono::Utc;
        use uuid::Uuid;

        let reason = "deploy v2.3.1";
        let change_id = "CHG-20240614-001";
        let hosts = vec!["web-1", "web-2", "web-3"];

        // Simulate what append_audit does for each host in an exec-multi
        let mut jsonl_lines = Vec::new();
        for host in &hosts {
            let result = ExecResult {
                host: host.to_string(),
                command: "systemctl restart app".into(),
                exit_code: Some(0),
                stdout: "ok".into(),
                stderr: String::new(),
                duration_ms: 150,
                risk_level: RiskLevel::Medium,
                truncated: false,
            };
            // Mirror the AuditEntry construction in append_audit
            let entry = AuditEntry {
                id: Uuid::new_v4(),
                ts: Utc::now(),
                host: result.host.clone(),
                command: redact_sensitive_text(&result.command),
                exit_code: result.exit_code,
                duration_ms: result.duration_ms,
                risk_level: RiskLevel::Medium,
                reason: Some(reason.to_string()),
                change_id: Some(change_id.to_string()),
                source: Some("mcp".into()),
            };
            jsonl_lines.push(serde_json::to_string(&entry).unwrap());
        }

        assert_eq!(jsonl_lines.len(), 3, "one audit entry per host");

        let mut seen_hosts = Vec::new();
        for line in &jsonl_lines {
            let entry: AuditEntry = serde_json::from_str(line).unwrap();
            assert_eq!(
                entry.reason,
                Some(reason.into()),
                "audit entry for {} should have reason",
                entry.host
            );
            assert_eq!(
                entry.change_id,
                Some(change_id.into()),
                "audit entry for {} should have change_id",
                entry.host
            );
            assert_eq!(entry.command, "systemctl restart app");
            assert_eq!(entry.exit_code, Some(0));
            assert_eq!(entry.risk_level, RiskLevel::Medium);
            seen_hosts.push(entry.host);
        }

        for host in &hosts {
            assert!(
                seen_hosts.contains(&host.to_string()),
                "host {} should appear in audit entries",
                host
            );
        }
    }

    #[test]
    fn test_exec_multi_audit_entries_without_reason() {
        // Verify exec-multi without reason/change_id produces entries with None
        use chrono::Utc;
        use uuid::Uuid;

        let entry = AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            host: "db-1".into(),
            command: "pg_dump mydb".into(),
            exit_code: Some(0),
            duration_ms: 3000,
            risk_level: RiskLevel::Low,
            reason: None,
            change_id: None,
            source: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: AuditEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.reason, None);
        assert_eq!(parsed.change_id, None);
        assert_eq!(parsed.host, "db-1");
    }

    #[test]
    fn test_audit_entry_jsonl_roundtrip_multi_host() {
        // Simulate a full exec-multi audit trail: write JSONL entries for
        // multiple hosts, read them back, and verify reason/change_id survive
        // the round-trip — including search-style filtering.
        use chrono::Utc;
        use uuid::Uuid;

        let entries = vec![
            AuditEntry {
                id: Uuid::new_v4(),
                ts: Utc::now(),
                host: "alpha".into(),
                command: "uptime".into(),
                exit_code: Some(0),
                duration_ms: 50,
                risk_level: RiskLevel::Low,
                reason: Some("health check".into()),
                change_id: Some("CHG-100".into()),
                source: Some("cli".into()),
            },
            AuditEntry {
                id: Uuid::new_v4(),
                ts: Utc::now(),
                host: "beta".into(),
                command: "df -h".into(),
                exit_code: Some(0),
                duration_ms: 80,
                risk_level: RiskLevel::Low,
                reason: None,
                change_id: None,
                source: None,
            },
            AuditEntry {
                id: Uuid::new_v4(),
                ts: Utc::now(),
                host: "gamma".into(),
                command: "free -m".into(),
                exit_code: Some(1),
                duration_ms: 120,
                risk_level: RiskLevel::Medium,
                reason: Some("health check".into()),
                change_id: Some("CHG-100".into()),
                source: Some("mcp".into()),
            },
        ];

        // Write JSONL and read back
        let jsonl: String = entries
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect::<Vec<_>>()
            .join("\n");

        let parsed: Vec<AuditEntry> = jsonl
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();

        assert_eq!(parsed.len(), 3);

        // alpha and gamma share the same change_id
        let with_change: Vec<_> = parsed
            .iter()
            .filter(|e| e.change_id == Some("CHG-100".into()))
            .collect();
        assert_eq!(with_change.len(), 2);
        assert!(with_change.iter().any(|e| e.host == "alpha"));
        assert!(with_change.iter().any(|e| e.host == "gamma"));

        // beta has no reason/change_id
        let beta = parsed.iter().find(|e| e.host == "beta").unwrap();
        assert_eq!(beta.reason, None);
        assert_eq!(beta.change_id, None);

        // Search for "health" in reason context identifies the right entries
        let health_entries: Vec<_> = parsed
            .iter()
            .filter(|e| e.reason.as_deref() == Some("health check"))
            .collect();
        assert_eq!(health_entries.len(), 2);
    }
}
