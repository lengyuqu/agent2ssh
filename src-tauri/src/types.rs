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
    /// Another host profile alias to use as a ProxyJump (-J) bastion.
    #[serde(default)]
    pub jump_host: Option<String>,
    /// Override risk level for all commands on this host (e.g. "low" to skip confirmations).
    #[serde(default)]
    pub risk_override: Option<RiskLevel>,
    /// Tags for grouping hosts (e.g. ["production", "web"])
    #[serde(default)]
    pub tags: Vec<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecMultiResult {
    pub host: String,
    pub result: Option<ExecResult>,
    pub error: Option<String>,
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
}

fn default_risk() -> RiskLevel {
    RiskLevel::Low
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
