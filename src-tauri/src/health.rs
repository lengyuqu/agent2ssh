use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;

use crate::connection::ssh_target;
use crate::core::build_ssh_command;
use crate::store::{config_dir, load_config};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostHealthSnapshot {
    pub host: String,
    pub reachable: bool,
    pub latency_ms: Option<u64>,
    pub uptime: Option<String>,
    pub load_avg: Option<String>,
    pub disk_usage: Option<String>,
    pub memory_usage: Option<String>,
    pub collected_at: chrono::DateTime<Utc>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthSnapshot {
    pub hosts: Vec<HostHealthSnapshot>,
    pub collected_at: chrono::DateTime<Utc>,
    pub total_ms: u128,
}

const HEALTH_COMMAND: &str = "echo \"===UPTIME===\"; uptime; echo \"===DISK===\"; df -h / 2>/dev/null | tail -1; echo \"===MEMORY===\"; free -m 2>/dev/null | head -2; echo \"===LOAD===\"; cat /proc/loadavg 2>/dev/null || uptime";

/// Collect health snapshots for all given hosts concurrently via SSH.
pub async fn collect_health_snapshot(
    hosts: Vec<String>,
    timeout_secs: Option<u64>,
) -> HealthSnapshot {
    let timeout = timeout_secs.unwrap_or(10);
    let started = Instant::now();
    let mut set = JoinSet::new();

    for name in hosts {
        set.spawn(async move {
            let collected_at = Utc::now();
            let host_profile = match load_config()
                .ok()
                .and_then(|c| c.hosts.into_iter().find(|h| h.name == name))
            {
                Some(h) => h,
                None => {
                    return HostHealthSnapshot {
                        host: name,
                        reachable: false,
                        latency_ms: None,
                        uptime: None,
                        load_avg: None,
                        disk_usage: None,
                        memory_usage: None,
                        collected_at,
                        error: Some("unknown host profile".into()),
                    };
                }
            };

            let target = ssh_target(&host_profile);
            let mut cmd = build_ssh_command(&host_profile);
            cmd.arg("-o")
                .arg(format!("ConnectTimeout={timeout}"))
                .arg(&target)
                .arg(HEALTH_COMMAND)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let conn_start = Instant::now();
            match tokio::time::timeout(Duration::from_secs(timeout + 5), cmd.output()).await {
                Ok(Ok(output)) if output.status.success() => {
                    let latency_ms = conn_start.elapsed().as_millis() as u64;
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let parsed = parse_health_output(&stdout);
                    HostHealthSnapshot {
                        host: name,
                        reachable: true,
                        latency_ms: Some(latency_ms),
                        uptime: parsed.uptime,
                        load_avg: parsed.load_avg,
                        disk_usage: parsed.disk_usage,
                        memory_usage: parsed.memory_usage,
                        collected_at,
                        error: None,
                    }
                }
                Ok(Ok(output)) => {
                    let latency_ms = conn_start.elapsed().as_millis() as u64;
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    HostHealthSnapshot {
                        host: name,
                        reachable: false,
                        latency_ms: Some(latency_ms),
                        uptime: None,
                        load_avg: None,
                        disk_usage: None,
                        memory_usage: None,
                        collected_at,
                        error: Some(format!("SSH command failed: {}", stderr.trim())),
                    }
                }
                Ok(Err(e)) => HostHealthSnapshot {
                    host: name,
                    reachable: false,
                    latency_ms: None,
                    uptime: None,
                    load_avg: None,
                    disk_usage: None,
                    memory_usage: None,
                    collected_at,
                    error: Some(e.to_string()),
                },
                Err(_) => HostHealthSnapshot {
                    host: name,
                    reachable: false,
                    latency_ms: None,
                    uptime: None,
                    load_avg: None,
                    disk_usage: None,
                    memory_usage: None,
                    collected_at,
                    error: Some(format!("timed out after {timeout}s")),
                },
            }
        });
    }

    let mut host_results = Vec::new();
    while let Some(joined) = set.join_next().await {
        host_results.push(joined.unwrap_or_else(|e| HostHealthSnapshot {
            host: "unknown".into(),
            reachable: false,
            latency_ms: None,
            uptime: None,
            load_avg: None,
            disk_usage: None,
            memory_usage: None,
            collected_at: Utc::now(),
            error: Some(format!("task panicked: {e}")),
        }));
    }

    let total_ms = started.elapsed().as_millis();
    let snapshot = HealthSnapshot {
        hosts: host_results,
        collected_at: Utc::now(),
        total_ms,
    };

    // Persist to disk
    if let Ok(path) = config_dir() {
        let snapshot_path = path.join("health_snapshot.json");
        if let Ok(json) = serde_json::to_string_pretty(&snapshot) {
            let _ = std::fs::write(&snapshot_path, json);
        }
    }

    snapshot
}

/// Load the last persisted health snapshot from disk.
pub fn load_health_snapshot() -> Result<HealthSnapshot> {
    let path = config_dir()?.join("health_snapshot.json");
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let snapshot: HealthSnapshot =
        serde_json::from_str(&content).with_context(|| "failed to parse health snapshot JSON")?;
    Ok(snapshot)
}

// ── Output parsing ─────────────────────────────────────────────────────────

struct ParsedHealth {
    uptime: Option<String>,
    load_avg: Option<String>,
    disk_usage: Option<String>,
    memory_usage: Option<String>,
}

fn parse_health_output(stdout: &str) -> ParsedHealth {
    let mut uptime = None;
    let mut disk_usage = None;
    let mut memory_usage = None;
    let mut load_avg = None;

    let mut current_section: Option<&str> = None;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed == "===UPTIME===" {
            current_section = Some("uptime");
            continue;
        } else if trimmed == "===DISK===" {
            current_section = Some("disk");
            continue;
        } else if trimmed == "===MEMORY===" {
            current_section = Some("memory");
            continue;
        } else if trimmed == "===LOAD===" {
            current_section = Some("load");
            continue;
        }

        if trimmed.is_empty() {
            continue;
        }

        match current_section {
            Some("uptime") if uptime.is_none() => {
                uptime = Some(trimmed.to_string());
            }
            Some("disk") if disk_usage.is_none() => {
                disk_usage = Some(trimmed.to_string());
            }
            Some("memory") => {
                // free -m produces a header line and a data line; accumulate both
                memory_usage = Some(match memory_usage {
                    Some(existing) => format!("{existing}\n{trimmed}"),
                    None => trimmed.to_string(),
                });
            }
            Some("load") if load_avg.is_none() => {
                // /proc/loadavg: "0.15 0.10 0.05 1/234 5678" -- take first 3 fields
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 3 {
                    load_avg = Some(format!("{}, {}, {}", parts[0], parts[1], parts[2]));
                } else {
                    // Fallback: the uptime command output
                    load_avg = Some(trimmed.to_string());
                }
            }
            _ => {}
        }
    }

    ParsedHealth {
        uptime,
        load_avg,
        disk_usage,
        memory_usage,
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_snapshot_json_roundtrip() {
        let snapshot = HealthSnapshot {
            hosts: vec![
                HostHealthSnapshot {
                    host: "web-1".into(),
                    reachable: true,
                    latency_ms: Some(42),
                    uptime: Some("up 30 days".into()),
                    load_avg: Some("0.15, 0.10, 0.05".into()),
                    disk_usage: Some("/dev/sda1  50G  20G  30G  40% /".into()),
                    memory_usage: Some("Mem:  16384  8192  8192".into()),
                    collected_at: Utc::now(),
                    error: None,
                },
                HostHealthSnapshot {
                    host: "db-1".into(),
                    reachable: false,
                    latency_ms: None,
                    uptime: None,
                    load_avg: None,
                    disk_usage: None,
                    memory_usage: None,
                    collected_at: Utc::now(),
                    error: Some("timed out after 10s".into()),
                },
            ],
            collected_at: Utc::now(),
            total_ms: 5000,
        };

        let json = serde_json::to_string_pretty(&snapshot).unwrap();
        let deserialized: HealthSnapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.hosts.len(), 2);
        assert_eq!(deserialized.hosts[0].host, "web-1");
        assert!(deserialized.hosts[0].reachable);
        assert_eq!(deserialized.hosts[0].latency_ms, Some(42));
        assert_eq!(deserialized.hosts[1].host, "db-1");
        assert!(!deserialized.hosts[1].reachable);
        assert_eq!(
            deserialized.hosts[1].error.as_deref(),
            Some("timed out after 10s")
        );
        assert_eq!(deserialized.total_ms, 5000);
    }

    #[test]
    fn test_host_health_snapshot_serialization() {
        let host = HostHealthSnapshot {
            host: "test-host".into(),
            reachable: true,
            latency_ms: Some(100),
            uptime: Some("up 5 days, 3:21".into()),
            load_avg: Some("1.23, 0.45, 0.67".into()),
            disk_usage: Some("/dev/sda1  100G  50G  50G  50% /".into()),
            memory_usage: Some("Mem:  32768  16384  16384".into()),
            collected_at: Utc::now(),
            error: None,
        };

        let json = serde_json::to_string(&host).unwrap();
        let deserialized: HostHealthSnapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.host, "test-host");
        assert!(deserialized.reachable);
        assert_eq!(deserialized.latency_ms, Some(100));
        assert_eq!(deserialized.uptime.as_deref(), Some("up 5 days, 3:21"));
        assert_eq!(deserialized.load_avg.as_deref(), Some("1.23, 0.45, 0.67"));
        assert!(deserialized.error.is_none());
    }

    #[test]
    fn test_parse_health_output() {
        let sample = r#"===UPTIME===
 14:32:01 up 30 days,  3:21,  1 user,  load average: 0.15, 0.10, 0.05
===DISK===
/dev/sda1        50G   20G   30G  40% /
===MEMORY===
              total        used        free      shared  buff/cache   available
Mem:          16384        8192        4096         256        4096        7936
===LOAD===
0.15 0.10 0.05 1/234 5678
"#;
        let parsed = parse_health_output(sample);
        assert!(parsed.uptime.is_some());
        assert!(parsed.uptime.as_ref().unwrap().contains("up 30 days"));
        assert!(parsed.disk_usage.is_some());
        assert!(parsed.disk_usage.as_ref().unwrap().contains("40%"));
        assert!(parsed.memory_usage.is_some());
        assert!(parsed.memory_usage.as_ref().unwrap().contains("Mem:"));
        assert_eq!(parsed.load_avg.as_deref(), Some("0.15, 0.10, 0.05"));
    }

    #[test]
    fn test_parse_health_output_empty() {
        let parsed = parse_health_output("");
        assert!(parsed.uptime.is_none());
        assert!(parsed.load_avg.is_none());
        assert!(parsed.disk_usage.is_none());
        assert!(parsed.memory_usage.is_none());
    }
}
