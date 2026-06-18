use crate::store::config_dir;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLimitConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_window_secs")]
    pub window_secs: u64,
    #[serde(default = "default_source_per_minute")]
    pub default_source_per_minute: usize,
    #[serde(default = "default_host_per_minute")]
    pub default_host_per_minute: usize,
    #[serde(default = "default_tag_per_minute")]
    pub default_tag_per_minute: usize,
    #[serde(default = "default_source_max_sessions")]
    pub default_source_max_sessions: usize,
    #[serde(default = "default_host_max_sessions")]
    pub default_host_max_sessions: usize,
    #[serde(default = "default_tag_max_sessions")]
    pub default_tag_max_sessions: usize,
    #[serde(default)]
    pub source: HashMap<String, ExecutionLimitRule>,
    #[serde(default)]
    pub host: HashMap<String, ExecutionLimitRule>,
    #[serde(default)]
    pub tag: HashMap<String, ExecutionLimitRule>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionLimitRule {
    #[serde(default)]
    pub per_minute: Option<usize>,
    #[serde(default)]
    pub max_sessions: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionLimitRejection {
    pub scope: String,
    pub limit: usize,
    pub current: usize,
}

#[derive(Debug, Clone)]
struct ExecutionEvent {
    at: Instant,
    source: String,
    host: String,
    tags: Vec<String>,
}

#[derive(Debug, Clone)]
struct SessionLimitRecord {
    source: String,
    host: String,
    tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ExecutionLimiter {
    config: ExecutionLimitConfig,
    events: Vec<ExecutionEvent>,
    sessions: HashMap<Uuid, SessionLimitRecord>,
}

impl ExecutionLimiter {
    pub fn new(config: ExecutionLimitConfig) -> Self {
        Self {
            config,
            events: Vec::new(),
            sessions: HashMap::new(),
        }
    }

    pub fn check_execution_batch(
        &mut self,
        source: &str,
        targets: &[(String, Vec<String>)],
    ) -> std::result::Result<(), ExecutionLimitRejection> {
        if !self.config.enabled || targets.is_empty() {
            return Ok(());
        }
        self.prune_old_events();
        let mut proposed = self.events.clone();
        let now = Instant::now();
        for (host, tags) in targets {
            proposed.push(ExecutionEvent {
                at: now,
                source: normalize_key(source),
                host: normalize_key(host),
                tags: normalize_keys(tags),
            });
        }
        self.check_event_limits(source, targets, &proposed)?;
        self.events = proposed;
        Ok(())
    }

    pub fn check_session_open(
        &self,
        source: &str,
        host: &str,
        tags: &[String],
    ) -> std::result::Result<(), ExecutionLimitRejection> {
        if !self.config.enabled {
            return Ok(());
        }
        let source_key = normalize_key(source);
        let host_key = normalize_key(host);
        let tag_keys = normalize_keys(tags);

        let source_limit = self
            .source_rule(&source_key)
            .max_sessions
            .unwrap_or(self.config.default_source_max_sessions);
        let source_current = self
            .sessions
            .values()
            .filter(|s| s.source == source_key)
            .count();
        if source_limit > 0 && source_current >= source_limit {
            return Err(ExecutionLimitRejection {
                scope: format!("source:{source} sessions"),
                limit: source_limit,
                current: source_current,
            });
        }

        let host_limit = self
            .host_rule(&host_key)
            .max_sessions
            .unwrap_or(self.config.default_host_max_sessions);
        let host_current = self
            .sessions
            .values()
            .filter(|s| s.host == host_key)
            .count();
        if host_limit > 0 && host_current >= host_limit {
            return Err(ExecutionLimitRejection {
                scope: format!("host:{host} sessions"),
                limit: host_limit,
                current: host_current,
            });
        }

        for tag in &tag_keys {
            let tag_limit = self
                .tag_rule(tag)
                .max_sessions
                .unwrap_or(self.config.default_tag_max_sessions);
            let tag_current = self
                .sessions
                .values()
                .filter(|s| s.tags.iter().any(|t| t == tag))
                .count();
            if tag_limit > 0 && tag_current >= tag_limit {
                return Err(ExecutionLimitRejection {
                    scope: format!("tag:{tag} sessions"),
                    limit: tag_limit,
                    current: tag_current,
                });
            }
        }

        Ok(())
    }

    pub fn try_register_session(
        &mut self,
        id: Uuid,
        source: &str,
        host: &str,
        tags: &[String],
    ) -> std::result::Result<(), ExecutionLimitRejection> {
        self.check_session_open(source, host, tags)?;
        self.register_session(id, source, host, tags);
        Ok(())
    }

    pub fn register_session(&mut self, id: Uuid, source: &str, host: &str, tags: &[String]) {
        self.sessions.insert(
            id,
            SessionLimitRecord {
                source: normalize_key(source),
                host: normalize_key(host),
                tags: normalize_keys(tags),
            },
        );
    }

    pub fn unregister_session(&mut self, id: &Uuid) {
        self.sessions.remove(id);
    }

    pub fn session_target(&self, id: &Uuid) -> Option<(String, Vec<String>)> {
        self.sessions
            .get(id)
            .map(|session| (session.host.clone(), session.tags.clone()))
    }

    fn check_event_limits(
        &self,
        source: &str,
        targets: &[(String, Vec<String>)],
        events: &[ExecutionEvent],
    ) -> std::result::Result<(), ExecutionLimitRejection> {
        let source_key = normalize_key(source);
        let source_limit = self
            .source_rule(&source_key)
            .per_minute
            .unwrap_or(self.config.default_source_per_minute);
        let source_count = events.iter().filter(|e| e.source == source_key).count();
        if source_limit > 0 && source_count > source_limit {
            return Err(ExecutionLimitRejection {
                scope: format!("source:{source} rate"),
                limit: source_limit,
                current: source_count - 1,
            });
        }

        for (host, tags) in targets {
            let host_key = normalize_key(host);
            let host_limit = self
                .host_rule(&host_key)
                .per_minute
                .unwrap_or(self.config.default_host_per_minute);
            let host_count = events.iter().filter(|e| e.host == host_key).count();
            if host_limit > 0 && host_count > host_limit {
                return Err(ExecutionLimitRejection {
                    scope: format!("host:{host} rate"),
                    limit: host_limit,
                    current: host_count - 1,
                });
            }

            for tag in normalize_keys(tags) {
                let tag_limit = self
                    .tag_rule(&tag)
                    .per_minute
                    .unwrap_or(self.config.default_tag_per_minute);
                let tag_count = events
                    .iter()
                    .filter(|e| e.tags.iter().any(|t| t == &tag))
                    .count();
                if tag_limit > 0 && tag_count > tag_limit {
                    return Err(ExecutionLimitRejection {
                        scope: format!("tag:{tag} rate"),
                        limit: tag_limit,
                        current: tag_count - 1,
                    });
                }
            }
        }

        Ok(())
    }

    fn prune_old_events(&mut self) {
        let window = Duration::from_secs(self.config.window_secs.max(1));
        let now = Instant::now();
        self.events
            .retain(|event| now.duration_since(event.at) <= window);
    }

    fn source_rule(&self, source: &str) -> ExecutionLimitRule {
        self.config.source.get(source).cloned().unwrap_or_default()
    }

    fn host_rule(&self, host: &str) -> ExecutionLimitRule {
        self.config.host.get(host).cloned().unwrap_or_default()
    }

    fn tag_rule(&self, tag: &str) -> ExecutionLimitRule {
        self.config.tag.get(tag).cloned().unwrap_or_default()
    }
}

impl Default for ExecutionLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            window_secs: default_window_secs(),
            default_source_per_minute: default_source_per_minute(),
            default_host_per_minute: default_host_per_minute(),
            default_tag_per_minute: default_tag_per_minute(),
            default_source_max_sessions: default_source_max_sessions(),
            default_host_max_sessions: default_host_max_sessions(),
            default_tag_max_sessions: default_tag_max_sessions(),
            source: HashMap::new(),
            host: HashMap::new(),
            tag: HashMap::new(),
        }
    }
}

pub fn load_execution_limits() -> Result<ExecutionLimitConfig> {
    let path = config_dir()?.join("execution_limits.toml");
    if !path.exists() {
        return Ok(ExecutionLimitConfig::default());
    }
    let raw = fs::read_to_string(&path)?;
    let mut config: ExecutionLimitConfig = toml::from_str(&raw)?;
    normalize_config_keys(&mut config);
    Ok(config)
}

fn normalize_config_keys(config: &mut ExecutionLimitConfig) {
    config.source = normalize_rule_map(std::mem::take(&mut config.source));
    config.host = normalize_rule_map(std::mem::take(&mut config.host));
    config.tag = normalize_rule_map(std::mem::take(&mut config.tag));
}

fn normalize_rule_map(
    rules: HashMap<String, ExecutionLimitRule>,
) -> HashMap<String, ExecutionLimitRule> {
    rules
        .into_iter()
        .filter_map(|(key, rule)| {
            let key = normalize_key(&key);
            (!key.is_empty()).then_some((key, rule))
        })
        .collect()
}

fn normalize_key(value: &str) -> String {
    value.trim().to_lowercase()
}

fn normalize_keys(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|v| normalize_key(v))
        .filter(|v| !v.is_empty())
        .collect()
}

fn default_true() -> bool {
    true
}
fn default_window_secs() -> u64 {
    60
}
fn default_source_per_minute() -> usize {
    30
}
fn default_host_per_minute() -> usize {
    20
}
fn default_tag_per_minute() -> usize {
    60
}
fn default_source_max_sessions() -> usize {
    4
}
fn default_host_max_sessions() -> usize {
    4
}
fn default_tag_max_sessions() -> usize {
    8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limiter_with_config(config: ExecutionLimitConfig) -> ExecutionLimiter {
        ExecutionLimiter::new(config)
    }

    #[test]
    fn config_rule_keys_are_normalized() {
        let mut config = ExecutionLimitConfig::default();
        config.source.insert(
            " MCP ".into(),
            ExecutionLimitRule {
                per_minute: Some(1),
                max_sessions: None,
            },
        );
        config.host.insert(
            "Web".into(),
            ExecutionLimitRule {
                per_minute: None,
                max_sessions: Some(2),
            },
        );
        config.tag.insert(
            "Prod".into(),
            ExecutionLimitRule {
                per_minute: Some(3),
                max_sessions: Some(4),
            },
        );

        normalize_config_keys(&mut config);

        assert!(config.source.contains_key("mcp"));
        assert!(config.host.contains_key("web"));
        assert!(config.tag.contains_key("prod"));
    }

    #[test]
    fn execution_rate_limit_blocks_source_overage() {
        let mut limiter = limiter_with_config(ExecutionLimitConfig {
            default_source_per_minute: 1,
            default_host_per_minute: 0,
            default_tag_per_minute: 0,
            ..Default::default()
        });
        let targets = vec![("web".to_string(), vec![])];
        assert!(limiter.check_execution_batch("mcp", &targets).is_ok());
        let err = limiter.check_execution_batch("mcp", &targets).unwrap_err();
        assert_eq!(err.scope, "source:mcp rate");
        assert_eq!(err.limit, 1);
    }

    #[test]
    fn host_rule_overrides_default_rate_limit() {
        let mut host = HashMap::new();
        host.insert(
            "web".into(),
            ExecutionLimitRule {
                per_minute: Some(1),
                max_sessions: None,
            },
        );
        let mut limiter = limiter_with_config(ExecutionLimitConfig {
            default_source_per_minute: 0,
            default_host_per_minute: 10,
            default_tag_per_minute: 0,
            host,
            ..Default::default()
        });
        let targets = vec![("web".to_string(), vec![])];
        assert!(limiter.check_execution_batch("mcp", &targets).is_ok());
        let err = limiter.check_execution_batch("cli", &targets).unwrap_err();
        assert_eq!(err.scope, "host:web rate");
    }

    #[test]
    fn session_limit_blocks_host_overage() {
        let mut limiter = limiter_with_config(ExecutionLimitConfig {
            default_source_max_sessions: 0,
            default_host_max_sessions: 1,
            default_tag_max_sessions: 0,
            ..Default::default()
        });
        limiter.register_session(Uuid::new_v4(), "mcp", "web", &[]);
        let err = limiter.check_session_open("cli", "web", &[]).unwrap_err();
        assert_eq!(err.scope, "host:web sessions");
    }
}
