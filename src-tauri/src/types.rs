use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Blocked,
}

impl RiskLevel {
    /// Return whichever of `self` and `other` represents the higher severity.
    pub fn max_severity(self, other: RiskLevel) -> RiskLevel {
        fn rank(r: RiskLevel) -> u8 {
            match r {
                RiskLevel::Low => 0,
                RiskLevel::Medium => 1,
                RiskLevel::High => 2,
                RiskLevel::Blocked => 3,
            }
        }
        if rank(self) >= rank(other) { self } else { other }
    }
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskLevel::Low => write!(f, "low"),
            RiskLevel::Medium => write!(f, "medium"),
            RiskLevel::High => write!(f, "high"),
            RiskLevel::Blocked => write!(f, "blocked"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostProfile {
    pub name: String,
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub key_path: Option<String>,
    /// Optional SSH password for password-based hosts.
    ///
    /// Stored in local config when provided. Prefer key-based auth for shared
    /// or production usage.
    #[serde(default)]
    pub password: Option<String>,
    /// Another host profile alias to use as a ProxyJump (-J) bastion.
    #[serde(default)]
    pub jump_host: Option<String>,
    /// Override risk level for all commands on this host (e.g. "low" to skip confirmations).
    #[serde(default)]
    pub risk_override: Option<RiskLevel>,
    /// Tags for grouping hosts (e.g. ["production", "web"])
    #[serde(default)]
    pub tags: Vec<String>,
    /// Environment label for grouping hosts (e.g. "prod", "staging").
    #[serde(default)]
    pub env: Option<String>,
    /// Role label for grouping hosts (e.g. "web", "db").
    #[serde(default)]
    pub role: Option<String>,
    /// Owner label for grouping hosts by team or person.
    #[serde(default)]
    pub owner: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostFilter {
    pub env: Option<String>,
    pub role: Option<String>,
    pub owner: Option<String>,
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecRequest {
    pub host: String,
    pub command: String,
    /// Required for High-risk commands; ignored for Low/Medium; Blocked always fails.
    #[serde(default)]
    pub force: bool,
    /// Kill the remote command after this many seconds (default 60).
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Data to pipe into the remote command's stdin. The pipe is closed after writing.
    #[serde(default)]
    pub stdin: Option<String>,
    /// Truncate stdout+stderr to this many bytes total (default 4 MiB).
    #[serde(default)]
    pub max_output_bytes: Option<usize>,
    /// Optional reason/note for this operation (for audit trail).
    #[serde(default)]
    pub reason: Option<String>,
    /// Optional change/ticket ID for this operation (for audit trail).
    #[serde(default)]
    pub change_id: Option<String>,
    /// Source that initiated the operation, such as cli, mcp, daemon, or desktop.
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecMultiResult {
    pub host: String,
    pub result: Option<ExecResult>,
    pub error: Option<String>,
}

/// Strategy for batch multi-host execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchStrategy {
    /// Maximum number of concurrent hosts (0 = unlimited)
    #[serde(default)]
    pub concurrency: Option<usize>,
    /// Stop after this many failures (0 = never stop)
    #[serde(default)]
    pub max_failures: Option<usize>,
    /// Execute in batches of this size, waiting for each batch to complete
    #[serde(default)]
    pub batch_size: Option<usize>,
    /// Pause between batches (seconds)
    #[serde(default)]
    pub pause_between_batches_secs: Option<u64>,
}

/// Aggregated result of a batched multi-host execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecMultiBatchResult {
    pub results: Vec<ExecMultiResult>,
    pub total_hosts: usize,
    pub successful: usize,
    pub failed: usize,
    pub skipped: usize,
    pub stopped_early: bool,
    pub batches_executed: usize,
    pub total_duration_ms: u128,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditFilter {
    pub host: Option<String>,
    pub risk_level: Option<RiskLevel>,
    pub exit_code: Option<i32>,
    /// ISO-8601 lower bound (inclusive)
    pub since: Option<String>,
    /// ISO-8601 upper bound (inclusive)
    pub until: Option<String>,
    #[serde(default = "default_audit_limit")]
    pub limit: usize,
    /// Full-text search across command, host, and other text fields.
    #[serde(default)]
    pub search: Option<String>,
    /// Command pattern (glob-style: *, ?)
    #[serde(default)]
    pub command_pattern: Option<String>,
    /// Host group: filter by env label
    #[serde(default)]
    pub host_env: Option<String>,
    /// Host group: filter by role label
    #[serde(default)]
    pub host_role: Option<String>,
    /// Host group: filter by owner label
    #[serde(default)]
    pub host_owner: Option<String>,
}

fn default_audit_limit() -> usize {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResult {
    pub host: String,
    pub reachable: bool,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResult {
    pub host: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
    pub risk_level: RiskLevel,
    /// True when output was cut short by max_output_bytes.
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: Uuid,
    pub ts: DateTime<Utc>,
    pub host: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    #[serde(default = "default_risk")]
    pub risk_level: RiskLevel,
    /// Optional reason/note for the operation
    #[serde(default)]
    pub reason: Option<String>,
    /// Optional change/ticket ID
    #[serde(default)]
    pub change_id: Option<String>,
    /// Source that initiated the operation, such as cli, mcp, daemon, or desktop.
    #[serde(default)]
    pub source: Option<String>,
}

fn default_risk() -> RiskLevel {
    RiskLevel::Low
}

pub fn source_from_env(default_source: &str) -> String {
    std::env::var("AGENT2SSH_SOURCE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_source.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub hosts: Vec<HostProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SftpUploadRequest {
    pub host: String,
    pub local_path: String,
    pub remote_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SftpDownloadRequest {
    pub host: String,
    pub remote_path: String,
    pub local_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SftpResult {
    pub host: String,
    pub local_path: String,
    pub remote_path: String,
    pub direction: SftpDirection,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SftpDirection {
    Upload,
    Download,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForwardDirection {
    Local,
    Remote,
}

impl std::fmt::Display for ForwardDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ForwardDirection::Local => write!(f, "local"),
            ForwardDirection::Remote => write!(f, "remote"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardRule {
    pub id: uuid::Uuid,
    pub host: String,
    pub direction: ForwardDirection,
    pub bind_port: u16,
    pub target_host: String,
    pub target_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub host: String,
    pub connected: bool,
    pub socket_path: Option<String>,
}
