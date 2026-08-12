use agent2ssh::approval::{
    check_approval_required, list_approval_policies, load_approval_policies,
    save_approval_policies, ApprovalPolicy,
};
use agent2ssh::daemon_control::{
    process_is_alive as daemon_process_is_alive, read_daemon_pid, remove_daemon_pid_file,
    start_daemon_background, terminate_process,
};
use agent2ssh::events::subscribe_events;
use agent2ssh::execution_control::{
    append_rejected_exec_audit, authorize_command_with_approval, command_authorization_target,
    expand_exec_authorization_targets, CommandAuthorizationError, CommandAuthorizationInput,
};
use agent2ssh::remote::{
    check_daemon_scope, check_daemon_version, diagnose_daemon, get_daemon, get_daemon_with_scope,
    get_daemons_unified_view, tags_for_remote_scope_check, PROTOCOL_VERSION,
};
use agent2ssh::store::{audit_path, compute_metrics_trend, restrict_file_to_owner, TrendPeriod};
use agent2ssh::{
    add_host_core, add_snippet, collect_health_snapshot, compare_exec_results, dry_run_playbook,
    effective_command_risk, exec_multi_core, exec_multi_with_strategy, exec_ssh_core,
    export_audit_csv, export_audit_jsonl, export_team_config, filter_hosts, import_ssh_config_core,
    import_team_config, list_audit_core, list_daemons_core, list_hosts_filtered_core,
    list_playbooks_core, load_snippets, ping_hosts_core, preview_exec, preview_exec_multi,
    remove_host_core, remove_snippet, run_playbook_core_with_source_and_approved_steps,
    sftp_download_core_with_source, sftp_ls_core_with_source, sftp_mkdir_core_with_source,
    sftp_stat_core_with_source, sftp_upload_core_with_source, source_from_transport,
    validate_policy_path, AuditFilter, BatchStrategy, ExecComparison, ExecMultiBatchRequest,
    ExecMultiRequest, ExecRequest, ExecutionGateStatus, ForwardDirection, ForwardRule, HostFilter,
    HostProfile, PolicyDecision, PolicyTestResult, RiskLevel, SftpDownloadRequest,
    SftpUploadRequest, Snippet, TeamConfigExport,
};
use anyhow::{Context, Result};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::engine::ArgValueCandidates;
use clap_complete::env::{Bash, EnvCompleter, Fish, Powershell, Zsh};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[path = "agent2ssh/completion.rs"]
mod agent2ssh_completion;

use agent2ssh_completion::{
    daemon_candidates, forward_candidates, host_candidates, playbook_candidates, session_candidates,
};

#[derive(Debug, Parser)]
#[command(name = "agent2ssh", version)]
#[command(about = "SSH capability layer for agents")]
struct Cli {
    /// Route operations through a remote daemon by alias (from ~/.agent2ssh/remotes.toml).
    /// Use "localhost" or omit for the local daemon.
    #[arg(long, global = true, add = ArgValueCandidates::new(daemon_candidates))]
    daemon: Option<String>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Generate dynamic shell completion registration
    Completions {
        /// Shell to generate registration for
        shell: CompletionShell,
    },
    Host {
        #[command(subcommand)]
        command: HostCommands,
    },
    Exec {
        #[arg(add = ArgValueCandidates::new(host_candidates))]
        host: String,
        command: String,
        #[arg(long)]
        json: bool,
        /// Required for high-risk commands
        #[arg(long)]
        force: bool,
        /// Kill the command after N seconds (default 60)
        #[arg(long)]
        timeout_secs: Option<u64>,
        /// Pipe this string into the remote command's stdin
        #[arg(long)]
        stdin: Option<String>,
        /// Show execution plan without running the command
        #[arg(long)]
        plan: bool,
        /// Optional reason/note for this operation (audit trail)
        #[arg(long)]
        reason: Option<String>,
        /// Optional change/ticket ID for this operation (audit trail)
        #[arg(long)]
        change_id: Option<String>,
    },
    /// Run the same command on multiple hosts concurrently
    ExecMulti {
        #[arg(required = true, num_args = 1.., add = ArgValueCandidates::new(host_candidates))]
        hosts: Vec<String>,
        #[arg(long)]
        command: String,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        timeout_secs: Option<u64>,
        #[arg(long, value_delimiter = ',')]
        tags: Option<Vec<String>>,
        /// Show execution plan without running the command
        #[arg(long)]
        plan: bool,
        /// Maximum number of concurrent hosts (0 = unlimited)
        #[arg(long)]
        concurrency: Option<usize>,
        /// Stop after this many failures (0 = never stop)
        #[arg(long)]
        max_failures: Option<usize>,
        /// Execute in batches of this size, waiting for each batch to complete
        #[arg(long)]
        batch_size: Option<usize>,
        /// Pause between batches in seconds
        #[arg(long)]
        pause_secs: Option<u64>,
        /// Show comparison of results across hosts
        #[arg(long)]
        compare: bool,
        /// Optional reason/note for this operation (audit trail)
        #[arg(long)]
        reason: Option<String>,
        /// Optional change/ticket ID for this operation (audit trail)
        #[arg(long)]
        change_id: Option<String>,
    },
    Sftp {
        #[command(subcommand)]
        command: SftpCommands,
    },
    Session {
        #[command(subcommand)]
        command: SessionCommands,
    },
    Forward {
        #[command(subcommand)]
        command: ForwardCommands,
    },
    /// Manage the app-managed encrypted credential store (master password)
    Secrets {
        #[command(subcommand)]
        command: SecretsCommands,
    },
    /// Check SSH reachability of one or more hosts
    Ping {
        #[arg(required = true, num_args = 1.., add = ArgValueCandidates::new(host_candidates))]
        hosts: Vec<String>,
        #[arg(long, default_value_t = 5)]
        timeout_secs: u64,
        #[arg(long)]
        json: bool,
    },
    Audit {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
        #[arg(long, add = ArgValueCandidates::new(host_candidates))]
        host: Option<String>,
        #[arg(long, value_enum)]
        risk: Option<RiskLevel>,
        #[arg(long)]
        exit_code: Option<i32>,
        /// ISO-8601 lower bound e.g. 2025-01-01T00:00:00Z
        #[arg(long)]
        since: Option<String>,
        /// ISO-8601 upper bound
        #[arg(long)]
        until: Option<String>,
        /// Full-text search across command and host fields
        #[arg(long)]
        search: Option<String>,
        /// Command pattern (glob-style: *, ?)
        #[arg(long)]
        command_pattern: Option<String>,
        /// Filter by host environment label
        #[arg(long)]
        env: Option<String>,
        /// Filter by host role label
        #[arg(long)]
        role: Option<String>,
        /// Filter by host owner label
        #[arg(long)]
        owner: Option<String>,
        /// Export format: jsonl or csv (writes to stdout or --output file)
        #[arg(long, value_name = "FORMAT")]
        format: Option<String>,
        /// Write output to file instead of stdout
        #[arg(long, value_name = "PATH")]
        output: Option<String>,
    },
    /// Check risk level of a command
    Risk {
        command: String,
        #[arg(long, add = ArgValueCandidates::new(host_candidates))]
        host: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Manage the daemon process
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
    },
    /// Pause non-desktop daemon execution through the global execution gate
    Pause {
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Optional reason/note stored with the gate state
        #[arg(long)]
        reason: Option<String>,
    },
    /// Resume daemon execution through the global execution gate
    Resume {
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Optional reason/note stored with the gate state
        #[arg(long)]
        reason: Option<String>,
    },
    /// Show the global execution gate status
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Export team configuration (hosts without keys, risk rules, playbooks)
    ConfigExport {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Import team configuration from a JSON file
    ConfigImport {
        /// Path to the JSON file to import
        path: String,
        /// Input is JSON format
        #[arg(long)]
        json: bool,
        /// Preview what will change without actually importing
        #[arg(long)]
        preview: bool,
    },
    /// Compare and sync Agent2SSH hosts with ~/.ssh/config
    SshSync {
        /// Show diff between Agent2SSH and ~/.ssh/config without changes
        #[arg(long)]
        diff: bool,
        /// Export Agent2SSH hosts to SSH config format
        #[arg(long)]
        export: bool,
        /// Path to SSH config file (default: ~/.ssh/config)
        #[arg(long)]
        path: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Sync portable Agent2SSH config through a WebDAV collection
    Webdav {
        #[command(subcommand)]
        command: WebDavCommands,
    },
    /// Manage reusable command snippets
    Snippet {
        #[command(subcommand)]
        command: SnippetCommands,
    },
    /// Run diagnostic checks on the agent2ssh environment
    Doctor {
        /// Output results as JSON
        #[arg(long)]
        json: bool,
        /// Run diagnostics against a specific remote daemon (by alias from remotes.toml)
        #[arg(long, add = ArgValueCandidates::new(daemon_candidates))]
        daemon: Option<String>,
    },
    /// Collect health snapshot (uptime, disk, memory, load) for configured hosts
    Health {
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Hosts to collect health from (default: all configured hosts)
        #[arg(long, add = ArgValueCandidates::new(host_candidates))]
        hosts: Option<Vec<String>>,
    },
    /// Manage approval policies for high-risk command authorization
    Policy {
        #[command(subcommand)]
        command: PolicyCommands,
    },
    /// Run or dry-run a named playbook (sequence of SSH commands)
    Playbook {
        #[command(subcommand)]
        command: PlaybookCommands,
    },
    /// Show execution metrics trends over time
    Metrics {
        #[command(subcommand)]
        command: MetricsCommands,
    },
    /// Subscribe to the real-time event stream
    Events {
        /// Output events as JSON lines
        #[arg(long)]
        json: bool,
    },
    /// Register agent2ssh with local AI agent clients (MCP config + Agent Skill)
    Integrate {
        #[command(subcommand)]
        command: IntegrateCommands,
    },
    /// B52: Check for app updates by querying GitHub releases
    VersionCheck {
        /// GitHub repo in owner/name format (default: lengyuqu/agent2ssh)
        #[arg(long)]
        repo: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    Powershell,
}

impl CompletionShell {
    fn completer(self) -> &'static dyn EnvCompleter {
        match self {
            Self::Bash => &Bash,
            Self::Zsh => &Zsh,
            Self::Fish => &Fish,
            Self::Powershell => &Powershell,
        }
    }
}

#[derive(Debug, Subcommand)]
enum IntegrateCommands {
    /// Show detection and registration status for all known agent clients
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Register the agent2ssh MCP server in a client's config (with backup)
    Add {
        /// Client id from `integrate list` (e.g. claude_code, cursor, codex)
        client: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove the agent2ssh MCP server entry from a client's config (with backup)
    Rm {
        /// Client id from `integrate list`
        client: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage the bundled Agent Skill (SKILL.md)
    Skill {
        #[command(subcommand)]
        command: IntegrateSkillCommands,
    },
}

#[derive(Debug, Subcommand)]
enum IntegrateSkillCommands {
    /// Show installed vs bundled skill version
    Status {
        /// Skill directory (default: ~/.claude/skills/agent2ssh)
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Install or update the bundled skill (same operation)
    Install {
        /// Skill directory (default: ~/.claude/skills/agent2ssh)
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove the installed skill
    Uninstall {
        /// Skill directory (default: ~/.claude/skills/agent2ssh)
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum HostCommands {
    List {
        #[arg(long)]
        env: Option<String>,
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Add {
        name: String,
        #[arg(long)]
        host: String,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        key: Option<String>,
        /// SSH password for password-based authentication. Prefer keys for production.
        #[arg(long)]
        password: Option<String>,
        /// Host profile alias to use as ProxyJump bastion
        #[arg(long, add = ArgValueCandidates::new(host_candidates))]
        jump: Option<String>,
        /// Override risk level for all commands on this host (low/medium/high)
        #[arg(long)]
        risk_override: Option<String>,
        /// Comma-separated tags for grouping
        #[arg(long, value_delimiter = ',')]
        tags: Option<Vec<String>>,
        /// Environment label for grouping hosts
        #[arg(long)]
        env: Option<String>,
        /// Role label for grouping hosts
        #[arg(long)]
        role: Option<String>,
        /// Owner label for grouping hosts
        #[arg(long)]
        owner: Option<String>,
        /// Command to execute on the remote shell immediately after connect
        #[arg(long)]
        init_command: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Rm {
        #[arg(add = ArgValueCandidates::new(host_candidates))]
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Import hosts from ~/.ssh/config (skips existing aliases)
    ImportConfig {
        /// Path to ssh config file (default: ~/.ssh/config)
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum SftpCommands {
    /// Upload a local file to the remote host
    Put {
        #[arg(add = ArgValueCandidates::new(host_candidates))]
        host: String,
        local: String,
        remote: String,
        /// Resume an interrupted upload by appending from the remote file's length (K6)
        #[arg(long)]
        resume: bool,
        #[arg(long)]
        json: bool,
    },
    /// Download a remote file to local path
    Get {
        #[arg(add = ArgValueCandidates::new(host_candidates))]
        host: String,
        remote: String,
        local: String,
        /// Resume an interrupted download by appending from the local file's length (K6)
        #[arg(long)]
        resume: bool,
        #[arg(long)]
        json: bool,
    },
    /// List a remote directory
    Ls {
        #[arg(add = ArgValueCandidates::new(host_candidates))]
        host: String,
        path: String,
        #[arg(long)]
        json: bool,
    },
    /// Stat a remote file or directory
    Stat {
        #[arg(add = ArgValueCandidates::new(host_candidates))]
        host: String,
        path: String,
        #[arg(long)]
        json: bool,
    },
    /// Create a remote directory (mkdir -p)
    Mkdir {
        #[arg(add = ArgValueCandidates::new(host_candidates))]
        host: String,
        path: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum SessionCommands {
    /// Open a persistent PTY session to a host
    Open {
        #[arg(add = ArgValueCandidates::new(host_candidates))]
        host: String,
        #[arg(long)]
        json: bool,
    },
    /// Write input to an open session
    Write {
        #[arg(add = ArgValueCandidates::new(session_candidates))]
        session_id: String,
        input: String,
    },
    /// Read buffered output from a session
    Read {
        #[arg(add = ArgValueCandidates::new(session_candidates))]
        session_id: String,
        #[arg(long, default_value_t = 2000)]
        timeout_ms: u64,
        #[arg(long)]
        json: bool,
    },
    /// Close a session
    Close {
        #[arg(add = ArgValueCandidates::new(session_candidates))]
        session_id: String,
        #[arg(long)]
        json: bool,
    },
    /// List open sessions
    List {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ForwardCommands {
    /// Start a port forward (SSH -L or -R tunnel)
    Add {
        #[arg(add = ArgValueCandidates::new(host_candidates))]
        host: String,
        /// local or remote
        #[arg(long, default_value = "local")]
        direction: String,
        #[arg(long)]
        bind_port: u16,
        #[arg(long)]
        target_host: String,
        #[arg(long)]
        target_port: u16,
        /// B68: Jump host (bastion) profile name to route the tunnel through.
        #[arg(long, add = ArgValueCandidates::new(host_candidates))]
        via: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Start multiple port forwards on a single host in one batch (B25)
    AddMulti {
        #[arg(add = ArgValueCandidates::new(host_candidates))]
        host: String,
        /// Repeatable: each occurrence adds one rule.
        /// Format: "direction:bind_port:target_host:target_port"
        /// direction is "local" or "remote"
        #[arg(long = "rule", num_args = 1)]
        rules: Vec<String>,
        /// B68: Jump host (bastion) profile name to route the tunnel through.
        #[arg(long, add = ArgValueCandidates::new(host_candidates))]
        via: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List active port forwards
    List {
        #[arg(long)]
        json: bool,
    },
    /// Stop a port forward by ID
    Rm {
        #[arg(add = ArgValueCandidates::new(forward_candidates))]
        id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum SecretsCommands {
    /// Show whether the credential store is initialized and unlocked
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Set (first time) or change the master password, re-encrypting stored
    /// credentials. Reads the password from --password, else AGENT2SSH_MASTER_PASSWORD,
    /// else a prompt on stdin.
    SetPassword {
        #[arg(long)]
        password: Option<String>,
    },
}

#[derive(Debug, Clone, Args)]
struct WebDavCliOptions {
    /// WebDAV collection URL. Also read from AGENT2SSH_WEBDAV_URL or webdav.toml.
    #[arg(long)]
    url: Option<String>,
    /// WebDAV username. Also read from AGENT2SSH_WEBDAV_USERNAME or webdav.toml.
    #[arg(long)]
    username: Option<String>,
    /// WebDAV password. Prefer --password-env or AGENT2SSH_WEBDAV_PASSWORD.
    #[arg(long)]
    password: Option<String>,
    /// Environment variable holding the WebDAV password.
    #[arg(long)]
    password_env: Option<String>,
    /// Path to a WebDAV config TOML file (default: ~/.agent2ssh/webdav.toml).
    #[arg(long)]
    config: Option<PathBuf>,
    /// Password for encrypting sync backups (AES-256-GCM + Argon2id).
    /// Also read from AGENT2SSH_SYNC_PASSWORD or webdav.toml.
    #[arg(long)]
    sync_password: Option<String>,
    /// Environment variable holding the sync encryption password.
    #[arg(long)]
    sync_password_env: Option<String>,
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

impl WebDavCliOptions {
    fn into_sync_options(self) -> agent2ssh::WebDavSyncOptions {
        agent2ssh::WebDavSyncOptions {
            url: self.url,
            username: self.username,
            password: self.password,
            password_env: self.password_env,
            config_path: self.config,
            sync_password: self.sync_password,
            sync_password_env: self.sync_password_env,
        }
    }
}

#[derive(Debug, Subcommand)]
enum WebDavCommands {
    /// Upload local syncable config to WebDAV and increment the global version
    Push {
        #[command(flatten)]
        options: WebDavCliOptions,
    },
    /// Download the latest WebDAV version after creating a local backup
    Pull {
        #[command(flatten)]
        options: WebDavCliOptions,
    },
    /// Show local and remote sync version markers
    Status {
        #[command(flatten)]
        options: WebDavCliOptions,
    },
}

#[derive(Debug, Serialize)]
struct SessionOpenRequest {
    host: String,
}

#[derive(Debug, Serialize)]
struct SessionWriteRequest {
    input: String,
    source: String,
}

#[derive(Debug, Serialize)]
struct GateUpdateRequest {
    source: String,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct OutputResponse {
    output: String,
}

#[derive(Debug, Deserialize)]
struct SessionListItem {
    id: String,
    host: String,
}

#[derive(Debug, Subcommand)]
enum DaemonCommands {
    /// Start the daemon in the background
    Start,
    /// Stop the running daemon
    Stop,
    /// Check if the daemon is running
    Status,
    /// Restart the daemon
    Restart,
    /// Rotate daemon token while the daemon is stopped
    RotateToken {
        #[arg(long)]
        json: bool,
    },
    /// List all configured daemons (localhost + remotes)
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show unified view of all daemons with health and metrics
    View {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum PolicyCommands {
    /// Validate the unified policy.toml or policy.json file
    Validate {
        /// Validate a specific policy file instead of ~/.agent2ssh/policy.toml
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Test the effective policy decision for a command
    Test {
        command: String,
        /// Host name used for approval policy host/tag matching
        #[arg(long, default_value = "localhost", add = ArgValueCandidates::new(host_candidates))]
        host: String,
        #[arg(long)]
        json: bool,
    },
    /// List all configured approval policies
    List {
        #[arg(long)]
        json: bool,
    },
    /// Add a new approval policy
    Add {
        /// Name for the new policy
        name: String,
        /// Comma-separated host names this policy applies to
        #[arg(long, value_delimiter = ',', add = ArgValueCandidates::new(host_candidates))]
        hosts: Option<Vec<String>>,
        /// Comma-separated tags this policy applies to
        #[arg(long, value_delimiter = ',')]
        tags: Option<Vec<String>>,
        /// Minimum risk level that triggers approval (low/medium/high/blocked)
        #[arg(long)]
        min_risk: Option<String>,
        /// Glob command pattern that triggers approval (e.g. "kubectl delete *")
        #[arg(long)]
        command_pattern: Option<String>,
        /// Set to auto-approve instead of requiring approval
        #[arg(long)]
        auto_approve: bool,
        /// Custom TTL for approvals in seconds
        #[arg(long)]
        ttl_secs: Option<u64>,
        #[arg(long)]
        json: bool,
    },
    /// Remove an approval policy by name
    Remove {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Check if a command on a host would require approval
    Check {
        #[arg(add = ArgValueCandidates::new(host_candidates))]
        host: String,
        command: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum MetricsCommands {
    /// Show execution metrics trend report
    Trend {
        /// Time period: 24h, 7d, 30d, or all
        #[arg(long, default_value = "24h")]
        period: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum PlaybookCommands {
    /// List all configured playbooks
    List {
        #[arg(long)]
        json: bool,
    },
    /// Run a named playbook against a host
    Run {
        /// Playbook name to run
        #[arg(add = ArgValueCandidates::new(playbook_candidates))]
        name: String,
        /// Target host profile alias
        #[arg(long, add = ArgValueCandidates::new(host_candidates))]
        host: String,
        /// Required for high-risk steps
        #[arg(long)]
        force: bool,
        /// Parameters as key=value pairs (repeatable)
        #[arg(long = "params", value_name = "KEY=VALUE")]
        params: Option<Vec<String>>,
        /// Optional reason/note for this operation (audit trail)
        #[arg(long)]
        reason: Option<String>,
        /// Optional change/ticket ID for this operation (audit trail)
        #[arg(long)]
        change_id: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show resolved commands without executing (dry run)
    DryRun {
        /// Playbook name to preview
        #[arg(add = ArgValueCandidates::new(playbook_candidates))]
        name: String,
        /// Parameters as key=value pairs (repeatable)
        #[arg(long = "params", value_name = "KEY=VALUE")]
        params: Option<Vec<String>>,
    },
}

#[derive(Debug, Subcommand)]
enum SnippetCommands {
    /// List saved command snippets
    List {
        #[arg(long)]
        json: bool,
    },
    /// Create or replace a command snippet
    Save {
        name: String,
        command: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Delete a command snippet by name
    Delete {
        name: String,
        #[arg(long)]
        json: bool,
    },
}

async fn effective_risk_for_policy(
    command: &str,
    risk_override: Option<RiskLevel>,
) -> (RiskLevel, bool) {
    let user_risk = agent2ssh::risk_config::classify_with_user_rules(command).await;
    let risk =
        agent2ssh::core::apply_risk_override(effective_command_risk(command).await, risk_override);
    (risk, user_risk.is_some())
}

async fn test_policy_decision(host: &str, command: &str) -> Result<PolicyTestResult> {
    let target = command_authorization_target(host);
    let (risk, matched_user_rule) = effective_risk_for_policy(command, target.risk_override).await;
    let approval = check_approval_required(host, &target.tags, command, risk)?;
    let decision = if risk == RiskLevel::Blocked {
        PolicyDecision::Block
    } else if risk == RiskLevel::High || approval.is_some() {
        PolicyDecision::Approve
    } else {
        PolicyDecision::Allow
    };

    Ok(PolicyTestResult {
        command: command.to_string(),
        host: host.to_string(),
        risk_level: risk,
        decision,
        matched_approval_policy: approval.map(|policy| policy.name),
        matched_user_rule,
    })
}

fn cli_host_tags(host: &str) -> Vec<String> {
    list_hosts_filtered_core(&HostFilter::default())
        .unwrap_or_default()
        .into_iter()
        .find(|h| h.name == host)
        .map(|h| h.tags)
        .unwrap_or_default()
}

async fn authorize_local_exec_request(req: &mut ExecRequest) -> Result<RiskLevel> {
    let target = command_authorization_target(&req.host);
    let source = req.source.as_deref().unwrap_or("cli").to_string();
    let auth_scope = None;
    let result = authorize_command_with_approval(
        CommandAuthorizationInput {
            auth_scope: &auth_scope,
            source: &source,
            host: &req.host,
            tags: &target.tags,
            risk_override: target.risk_override,
            command: &req.command,
            force: req.force,
            reason: req.reason.clone(),
            change_id: req.change_id.clone(),
        },
        |prompt| async move {
            let message = "approval required but no local approval handler is available";
            append_rejected_exec_audit(
                &prompt.source,
                &prompt.host,
                &prompt.command,
                prompt.risk,
                message,
                prompt.change_id.as_deref(),
            );
            Err(format!(
                "{message}; run through the daemon approval flow or use --force when policy allows"
            ))
        },
    )
    .await
    .map_err(command_authorization_error)?;
    if result.approved && result.risk == RiskLevel::High {
        req.force = true;
    }
    Ok(result.risk)
}

async fn authorize_local_exec_targets(
    hosts: &[String],
    tags: &Option<Vec<String>>,
    command: &str,
    force: bool,
    reason: Option<String>,
    change_id: Option<String>,
    source: &str,
) -> Result<Vec<String>> {
    let targets = expand_exec_authorization_targets(hosts, tags)?;
    let auth_scope = None;
    let mut approved_hosts = Vec::new();
    for target in targets {
        let result = authorize_command_with_approval(
            CommandAuthorizationInput {
                auth_scope: &auth_scope,
                source,
                host: &target.host,
                tags: &target.tags,
                risk_override: target.risk_override,
                command,
                force,
                reason: reason.clone(),
                change_id: change_id.clone(),
            },
            |prompt| async move {
                let message = "approval required but no local approval handler is available";
                append_rejected_exec_audit(
                    &prompt.source,
                    &prompt.host,
                    &prompt.command,
                    prompt.risk,
                    message,
                    prompt.change_id.as_deref(),
                );
                Err(format!(
                    "{message}; run through the daemon approval flow or use --force when policy allows"
                ))
            },
        )
        .await
        .map_err(command_authorization_error)?;
        if result.approved && result.risk == RiskLevel::High {
            approved_hosts.push(target.host);
        }
    }
    Ok(approved_hosts)
}

async fn authorize_local_playbook_run(
    playbook: &str,
    host: &str,
    force: bool,
    params: &HashMap<String, String>,
    reason: Option<String>,
    change_id: Option<String>,
    source: &str,
) -> Result<Vec<usize>> {
    let dry_run = dry_run_playbook(playbook, params)?;
    let target = command_authorization_target(host);
    let playbook_risk_override = list_playbooks_core()?
        .into_iter()
        .find(|item| item.name == playbook)
        .and_then(|item| item.risk_override);
    let risk_override = playbook_risk_override.or(target.risk_override);
    let auth_scope = None;
    let mut approved_steps = Vec::new();

    for step in dry_run.steps {
        let result = authorize_command_with_approval(
            CommandAuthorizationInput {
                auth_scope: &auth_scope,
                source,
                host,
                tags: &target.tags,
                risk_override,
                command: &step.command_resolved,
                force,
                reason: reason.clone(),
                change_id: change_id.clone(),
            },
            |prompt| async move {
                let message = "approval required but no local approval handler is available";
                append_rejected_exec_audit(
                    &prompt.source,
                    &prompt.host,
                    &prompt.command,
                    prompt.risk,
                    message,
                    prompt.change_id.as_deref(),
                );
                Err(format!(
                    "{message}; run through the daemon approval flow or use --force when policy allows"
                ))
            },
        )
        .await
        .map_err(command_authorization_error)?;
        if result.approved && result.risk == RiskLevel::High {
            approved_steps.push(step.step);
        }
    }

    Ok(approved_steps)
}

async fn authorize_local_operation(
    host: &str,
    command: &str,
    force: bool,
    source: &str,
) -> Result<()> {
    let target = command_authorization_target(host);
    let auth_scope = None;
    authorize_command_with_approval(
        CommandAuthorizationInput {
            auth_scope: &auth_scope,
            source,
            host,
            tags: &target.tags,
            risk_override: target.risk_override,
            command,
            force,
            reason: None,
            change_id: None,
        },
        |prompt| async move {
            let message = "approval required but no local approval handler is available";
            append_rejected_exec_audit(
                &prompt.source,
                &prompt.host,
                &prompt.command,
                prompt.risk,
                message,
                prompt.change_id.as_deref(),
            );
            Err(format!(
                "{message}; run through the daemon approval flow or use --force when policy allows"
            ))
        },
    )
    .await
    .map_err(command_authorization_error)?;
    Ok(())
}

fn command_authorization_error(error: CommandAuthorizationError) -> anyhow::Error {
    match error {
        CommandAuthorizationError::ScopeDenied(message) => anyhow::anyhow!(message),
        CommandAuthorizationError::Blocked { message, .. } => anyhow::anyhow!(message),
        CommandAuthorizationError::ApprovalRejected => {
            anyhow::anyhow!("command rejected by approver")
        }
        CommandAuthorizationError::ApprovalTimedOut => {
            anyhow::anyhow!("approval request timed out")
        }
        CommandAuthorizationError::Internal(message) => anyhow::anyhow!(message),
    }
}

async fn remote_snippet_list(alias: &str) -> Result<Vec<Snippet>> {
    let (url, token) = get_daemon(alias)?;
    let client = reqwest::Client::new();
    let mut request = client.get(format!("{}/snippets", url.trim_end_matches('/')));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    request
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .context("failed to decode daemon snippets response")
}

async fn remote_snippet_save(alias: &str, snippet: &Snippet) -> Result<Vec<Snippet>> {
    let (url, token) = get_daemon(alias)?;
    let client = reqwest::Client::new();
    let mut request = client
        .post(format!("{}/snippets", url.trim_end_matches('/')))
        .json(snippet);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    request
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .context("failed to decode daemon snippet save response")
}

async fn remote_snippet_delete(alias: &str, name: &str) -> Result<bool> {
    let (url, token) = get_daemon(alias)?;
    let mut endpoint = reqwest::Url::parse(&format!("{}/snippets/", url.trim_end_matches('/')))?;
    endpoint
        .path_segments_mut()
        .map_err(|_| anyhow::anyhow!("daemon URL cannot be used as a base URL"))?
        .push(name);
    let client = reqwest::Client::new();
    let mut request = client.delete(endpoint);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    request
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .context("failed to decode daemon snippet delete response")
}

fn print_completion_registration(shell: CompletionShell) -> Result<()> {
    shell
        .completer()
        .write_registration(
            "COMPLETE",
            "agent2ssh",
            "agent2ssh",
            "agent2ssh",
            &mut std::io::stdout().lock(),
        )
        .context("failed to generate shell completion registration")
}

fn main() -> Result<()> {
    // Dynamic shell completion must run before normal startup. In particular,
    // it must not migrate secrets, create ~/.agent2ssh, or start the daemon.
    clap_complete::CompleteEnv::with_factory(Cli::command)
        .bin("agent2ssh")
        .completer("agent2ssh")
        .complete();

    // Run the CLI future itself on a larger stack. Configuring Tokio's worker
    // stack alone is insufficient because Runtime::block_on polls the future
    // on the calling thread, whose default Windows stack is too small for the
    // unoptimized clap-generated state machine in debug builds.
    const CLI_STACK_SIZE: usize = 8 * 1024 * 1024;
    std::thread::Builder::new()
        .name("agent2ssh-cli".to_string())
        .stack_size(CLI_STACK_SIZE)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(CLI_STACK_SIZE)
                .build()?;
            runtime.block_on(async_main())
        })?
        .join()
        .map_err(|_| anyhow::anyhow!("CLI runtime thread panicked"))?
}

async fn async_main() -> Result<()> {
    agent2ssh::install_panic_hook("cli");
    agent2ssh::seed_trace_id_from_env();
    let cli = Cli::parse();

    if let Commands::Completions { shell } = &cli.command {
        print_completion_registration(*shell)?;
        return Ok(());
    }

    // K1: best-effort one-shot migration of any legacy plaintext passwords into
    // the app-managed encrypted store. No-op once clean; never blocks startup
    // on failure.
    if let Err(e) = agent2ssh::migrate_plaintext_secrets() {
        eprintln!("warning: secret migration skipped: {e}");
    }
    let daemon_alias = cli.daemon.clone();

    match cli.command {
        Commands::Completions { .. } => unreachable!("handled before CLI startup"),
        Commands::Host { command } => match command {
            HostCommands::List {
                env,
                role,
                owner,
                tag,
                json,
            } => {
                let filter = HostFilter {
                    env,
                    role,
                    owner,
                    tag,
                };
                // If --daemon is set and remote, forward via HTTP
                if let Some(ref alias) = daemon_alias {
                    if alias != "localhost" {
                        let (url, token) = get_daemon(alias)?;
                        let client = reqwest::Client::new();
                        let mut req = client.get(format!("{}/hosts", url.trim_end_matches('/')));
                        if let Some(ref t) = token {
                            req = req.bearer_auth(t);
                        }
                        let resp = req.send().await?;
                        let hosts: Vec<HostProfile> = resp.json().await?;
                        let hosts = filter_hosts(hosts, &filter);
                        if json {
                            println!("{}", serde_json::to_string_pretty(&hosts)?);
                        } else if hosts.is_empty() {
                            println!("No hosts configured on daemon '{alias}'.");
                        } else {
                            for host in hosts {
                                print_host_row(&host);
                            }
                        }
                        return Ok(());
                    }
                }
                let hosts: Vec<_> = list_hosts_filtered_core(&filter)?
                    .into_iter()
                    .map(HostProfile::redacted_for_transport)
                    .collect();
                if json {
                    println!("{}", serde_json::to_string_pretty(&hosts)?);
                } else if hosts.is_empty() {
                    println!("No hosts configured.");
                } else {
                    for host in hosts {
                        print_host_row(&host);
                    }
                }
            }
            HostCommands::Add {
                name,
                host,
                user,
                port,
                key,
                password,
                jump,
                risk_override,
                tags,
                env,
                role,
                owner,
                init_command,
                json,
            } => {
                let risk_override = risk_override.and_then(|s| match s.to_lowercase().as_str() {
                    "low" => Some(RiskLevel::Low),
                    "medium" => Some(RiskLevel::Medium),
                    "high" => Some(RiskLevel::High),
                    "blocked" => Some(RiskLevel::Blocked),
                    _ => None,
                });
                let profile = add_host_core(HostProfile {
                    name,
                    host,
                    user,
                    port,
                    key_path: key,
                    password: clean_optional(password),
                    jump_host: jump,
                    proxy_id: None,
                    risk_override,
                    tags: tags.unwrap_or_default(),
                    group: agent2ssh::default_host_group(),
                    env: clean_optional(env),
                    role: clean_optional(role),
                    owner: clean_optional(owner),
                    init_command: clean_optional(init_command),
                    passphrase: None,
                })?
                .redacted_for_transport();
                if json {
                    println!("{}", serde_json::to_string_pretty(&profile)?);
                } else {
                    println!("Saved host '{}'.", profile.name);
                }
            }
            HostCommands::Rm { name, json } => {
                remove_host_core(&name)?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({ "removed": name }))?
                    );
                } else {
                    println!("Removed host '{name}'.");
                }
            }
            HostCommands::ImportConfig { path, json } => {
                let added = import_ssh_config_core(path.as_deref())?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&added)?);
                } else if added.is_empty() {
                    println!("No new hosts imported (all aliases already exist).");
                } else {
                    println!("Imported {} host(s):", added.len());
                    for h in &added {
                        println!("  {} → {}:{}", h.name, h.host, h.port.unwrap_or(22));
                    }
                }
            }
        },
        Commands::Exec {
            host,
            command,
            json,
            force,
            timeout_secs,
            stdin,
            plan,
            reason,
            change_id,
        } => {
            // --plan: show execution plan without running
            if plan {
                let plan_result = preview_exec(&host, &command, timeout_secs).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&plan_result)?);
                } else {
                    print_exec_plan(&plan_result);
                }
                return Ok(());
            }

            let mut req = ExecRequest {
                host,
                command,
                force,
                timeout_secs,
                stdin,
                max_output_bytes: None,
                reason,
                change_id,
                source: Some(source_from_transport()),
            };

            // If --daemon is set and remote, forward via HTTP
            if let Some(ref alias) = daemon_alias {
                if alias != "localhost" {
                    let (url, token, scope) = get_daemon_with_scope(alias)?;
                    let local_tags = cli_host_tags(&req.host);
                    let token_val =
                        token.ok_or_else(|| anyhow::anyhow!("no token for daemon '{alias}'"))?;
                    let remote_tags = tags_for_remote_scope_check(
                        &scope, &url, &token_val, &req.host, local_tags,
                    )
                    .await?;
                    check_daemon_scope(&scope, &req.host, &remote_tags, &req.command)
                        .map_err(anyhow::Error::msg)?;
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(
                            req.timeout_secs.unwrap_or(60) + 10,
                        ))
                        .build()?;
                    let resp = client
                        .post(format!("{}/exec", url.trim_end_matches('/')))
                        .bearer_auth(&token_val)
                        .json(&req)
                        .send()
                        .await?;
                    if !resp.status().is_success() {
                        let body = resp.text().await.unwrap_or_default();
                        eprintln!("Remote daemon error: {body}");
                        std::process::exit(1);
                    }
                    let result: agent2ssh::types::ExecResult = resp.json().await?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&result)?);
                    } else {
                        print!("{}", result.stdout);
                        eprint!("{}", result.stderr);
                        std::process::exit(result.exit_code.unwrap_or(1));
                    }
                    return Ok(());
                }
            }

            if json {
                authorize_local_exec_request(&mut req).await?;
                let result = exec_ssh_core(req).await?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                let risk = authorize_local_exec_request(&mut req).await?;
                let risk_label = match risk {
                    RiskLevel::Low => "",
                    RiskLevel::Medium => " [risk: medium]",
                    RiskLevel::High => " [risk: high]",
                    RiskLevel::Blocked => " [risk: BLOCKED]",
                };
                if !risk_label.is_empty() {
                    eprintln!("agent2ssh:{}", risk_label);
                }
                let result = exec_ssh_core(req).await?;
                print!("{}", result.stdout);
                eprint!("{}", result.stderr);
                std::process::exit(result.exit_code.unwrap_or(1));
            }
        }
        Commands::ExecMulti {
            hosts,
            command,
            json,
            force,
            timeout_secs,
            tags,
            plan,
            concurrency,
            max_failures,
            batch_size,
            pause_secs,
            compare,
            reason,
            change_id,
        } => {
            // --plan: show execution plan without running
            if plan {
                let plan_result = preview_exec_multi(hosts, &command, tags, timeout_secs).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&plan_result)?);
                } else {
                    print_exec_plan(&plan_result);
                }
                return Ok(());
            }

            // Check if any strategy options are set
            let has_strategy = concurrency.is_some()
                || max_failures.is_some()
                || batch_size.is_some()
                || pause_secs.is_some();
            let source = source_from_transport();
            let approved_hosts = authorize_local_exec_targets(
                &hosts,
                &tags,
                &command,
                force,
                reason.clone(),
                change_id.clone(),
                &source,
            )
            .await?;

            if has_strategy {
                let strategy = BatchStrategy {
                    concurrency,
                    max_failures,
                    batch_size,
                    pause_between_batches_secs: pause_secs,
                };
                let batch_result = exec_multi_with_strategy(ExecMultiBatchRequest {
                    request: ExecMultiRequest {
                        hosts,
                        command,
                        force,
                        approved_hosts: approved_hosts.clone(),
                        timeout_secs,
                        tags,
                        reason,
                        change_id,
                        source: Some(source),
                    },
                    strategy: Some(strategy),
                })
                .await;
                if json {
                    println!("{}", serde_json::to_string_pretty(&batch_result)?);
                } else {
                    for r in &batch_result.results {
                        match &r.result {
                            Some(res) => println!(
                                "[{}] exit={:?} {}ms\n{}",
                                r.host,
                                res.exit_code,
                                res.duration_ms,
                                res.stdout.trim_end()
                            ),
                            None => eprintln!(
                                "[{}] ERROR: {}",
                                r.host,
                                r.error.as_deref().unwrap_or("unknown")
                            ),
                        }
                    }
                    println!(
                        "\n--- Batch Summary ---\n\
                         Total: {} | Success: {} | Failed: {} | Skipped: {}\n\
                         Batches: {} | Stopped early: {} | Duration: {}ms",
                        batch_result.total_hosts,
                        batch_result.successful,
                        batch_result.failed,
                        batch_result.skipped,
                        batch_result.batches_executed,
                        batch_result.stopped_early,
                        batch_result.total_duration_ms,
                    );
                }
                if compare {
                    let comparison = compare_exec_results(&batch_result.results);
                    if json {
                        println!("{}", serde_json::to_string_pretty(&comparison)?);
                    } else {
                        print_comparison(&comparison);
                    }
                }
            } else {
                let results = exec_multi_core(ExecMultiRequest {
                    hosts,
                    command,
                    force,
                    approved_hosts,
                    timeout_secs,
                    tags,
                    reason,
                    change_id,
                    source: Some(source),
                })
                .await;
                if json {
                    println!("{}", serde_json::to_string_pretty(&results)?);
                } else {
                    for r in &results {
                        match &r.result {
                            Some(res) => println!(
                                "[{}] exit={:?} {}ms\n{}",
                                r.host,
                                res.exit_code,
                                res.duration_ms,
                                res.stdout.trim_end()
                            ),
                            None => eprintln!(
                                "[{}] ERROR: {}",
                                r.host,
                                r.error.as_deref().unwrap_or("unknown")
                            ),
                        }
                    }
                }
                if compare {
                    let comparison = compare_exec_results(&results);
                    if json {
                        println!("{}", serde_json::to_string_pretty(&comparison)?);
                    } else {
                        print_comparison(&comparison);
                    }
                }
            }
        }
        Commands::Sftp { command } => match command {
            SftpCommands::Put {
                host,
                local,
                remote,
                resume,
                json,
            } => {
                let source = source_from_transport();
                let command = format!("sftp upload {} -> {}", local, remote);
                authorize_local_operation(&host, &command, false, &source).await?;
                let result = sftp_upload_core_with_source(
                    SftpUploadRequest {
                        host,
                        local_path: local,
                        remote_path: remote,
                        resume,
                        transfer_id: None,
                    },
                    Some(source),
                )
                .await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    println!(
                        "Uploaded '{}' → {}:{} ({}ms)",
                        result.local_path, result.host, result.remote_path, result.duration_ms
                    );
                }
            }
            SftpCommands::Get {
                host,
                remote,
                local,
                resume,
                json,
            } => {
                let source = source_from_transport();
                let command = format!("sftp download {} -> {}", remote, local);
                authorize_local_operation(&host, &command, false, &source).await?;
                let result = sftp_download_core_with_source(
                    SftpDownloadRequest {
                        host,
                        remote_path: remote,
                        local_path: local,
                        resume,
                        transfer_id: None,
                    },
                    Some(source),
                )
                .await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    println!(
                        "Downloaded {}:{} → '{}' ({}ms)",
                        result.host, result.remote_path, result.local_path, result.duration_ms
                    );
                }
            }
            SftpCommands::Ls { host, path, json } => {
                let source = source_from_transport();
                let command = format!("sftp ls {}", path);
                authorize_local_operation(&host, &command, false, &source).await?;
                let result = sftp_ls_core_with_source(&host, &path, None, Some(source)).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    print!("{}", result.stdout);
                }
            }
            SftpCommands::Stat { host, path, json } => {
                let source = source_from_transport();
                let command = format!("sftp stat {}", path);
                authorize_local_operation(&host, &command, false, &source).await?;
                let result = sftp_stat_core_with_source(&host, &path, None, Some(source)).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    print!("{}", result.stdout);
                }
            }
            SftpCommands::Mkdir { host, path, json } => {
                let source = source_from_transport();
                let command = format!("sftp mkdir {}", path);
                authorize_local_operation(&host, &command, false, &source).await?;
                let result = sftp_mkdir_core_with_source(&host, &path, None, Some(source)).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else if result.exit_code == Some(0) {
                    println!("Created directory '{path}' on {host}.");
                } else {
                    eprintln!("{}", result.stderr);
                    std::process::exit(result.exit_code.unwrap_or(1));
                }
            }
        },
        Commands::Session { command } => match command {
            SessionCommands::Open { host, json } => {
                let (client, base_url, token) = daemon_client(daemon_alias.as_deref())?;
                let result: IdResponse = daemon_json(
                    client
                        .post(format!("{base_url}/sessions"))
                        .bearer_auth(token)
                        .json(&SessionOpenRequest { host: host.clone() }),
                )
                .await?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(
                            &serde_json::json!({ "session_id": result.id, "host": host })
                        )?
                    );
                } else {
                    println!("Session opened: {}", result.id);
                }
            }
            SessionCommands::Write { session_id, input } => {
                let (client, base_url, token) = daemon_client(daemon_alias.as_deref())?;
                let _: serde_json::Value = daemon_json(
                    client
                        .post(format!("{base_url}/sessions/{session_id}/write"))
                        .bearer_auth(token)
                        .json(&SessionWriteRequest {
                            input,
                            source: source_from_transport(),
                        }),
                )
                .await?;
            }
            SessionCommands::Read {
                session_id,
                timeout_ms,
                json,
            } => {
                let (client, base_url, token) = daemon_client(daemon_alias.as_deref())?;
                let result: OutputResponse = daemon_json(
                    client
                        .get(format!("{base_url}/sessions/{session_id}/read"))
                        .bearer_auth(token)
                        .query(&[("timeout_ms", timeout_ms)]),
                )
                .await?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(
                            &serde_json::json!({ "output": result.output })
                        )?
                    );
                } else {
                    print!("{}", result.output);
                }
            }
            SessionCommands::Close { session_id, json } => {
                let (client, base_url, token) = daemon_client(daemon_alias.as_deref())?;
                let _: serde_json::Value = daemon_json(
                    client
                        .delete(format!("{base_url}/sessions/{session_id}"))
                        .bearer_auth(token),
                )
                .await?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({ "closed": session_id }))?
                    );
                } else {
                    println!("Session {session_id} closed.");
                }
            }
            SessionCommands::List { json } => {
                let (client, base_url, token) = daemon_client(daemon_alias.as_deref())?;
                let sessions: Vec<SessionListItem> = daemon_json(
                    client
                        .get(format!("{base_url}/sessions"))
                        .bearer_auth(token),
                )
                .await?;
                if json {
                    let items: Vec<_> = sessions
                        .iter()
                        .map(|item| serde_json::json!({ "session_id": item.id, "host": item.host }))
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&items)?);
                } else if sessions.is_empty() {
                    println!("No open sessions.");
                } else {
                    for item in &sessions {
                        println!("{}\t{}", item.id, item.host);
                    }
                }
            }
        },
        Commands::Forward { command } => match command {
            ForwardCommands::Add {
                host,
                direction,
                bind_port,
                target_host,
                target_port,
                via,
                json,
            } => {
                let dir = match direction.as_str() {
                    "local" => ForwardDirection::Local,
                    "remote" => ForwardDirection::Remote,
                    other => {
                        return Err(anyhow::anyhow!(
                            "direction must be 'local' or 'remote', got '{}'",
                            other
                        ))
                    }
                };
                let (client, base_url, token) = daemon_client(daemon_alias.as_deref())?;
                let mut request = client
                    .post(format!("{base_url}/forwards"))
                    .bearer_auth(token);
                if let Some(ref v) = via {
                    request = request.query(&[("via", v)]);
                }
                let rule: ForwardRule = daemon_json(request.json(&ForwardRule {
                    id: uuid::Uuid::new_v4(),
                    host,
                    direction: dir,
                    bind_port,
                    target_host,
                    target_port,
                }))
                .await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&rule)?);
                } else {
                    println!(
                        "Forward {} started: {}:{} → {}:{} (id={})",
                        rule.direction,
                        rule.bind_port,
                        rule.target_host,
                        rule.host,
                        rule.target_port,
                        rule.id
                    );
                }
            }
            ForwardCommands::AddMulti {
                host,
                rules,
                via,
                json,
            } => {
                // B25: Parse "direction:bind_port:target_host:target_port" strings.
                let mut parsed_rules = Vec::new();
                for (i, raw) in rules.iter().enumerate() {
                    let parts: Vec<&str> = raw.splitn(4, ':').collect();
                    if parts.len() != 4 {
                        return Err(anyhow::anyhow!(
                            "rule {}: expected format 'direction:bind_port:target_host:target_port', got '{}'",
                            i, raw
                        ));
                    }
                    let direction = match parts[0].to_lowercase().as_str() {
                        "remote" => ForwardDirection::Remote,
                        "local" => ForwardDirection::Local,
                        other => {
                            return Err(anyhow::anyhow!(
                                "rule {}: direction must be 'local' or 'remote', got '{}'",
                                i,
                                other
                            ))
                        }
                    };
                    let bind_port: u16 = parts[1].parse().map_err(|e| {
                        anyhow::anyhow!("rule {}: invalid bind_port '{}': {}", i, parts[1], e)
                    })?;
                    let target_host = parts[2].to_string();
                    let target_port: u16 = parts[3].parse().map_err(|e| {
                        anyhow::anyhow!("rule {}: invalid target_port '{}': {}", i, parts[3], e)
                    })?;
                    parsed_rules.push(agent2ssh::MultiForwardRule {
                        direction,
                        bind_port,
                        target_host,
                        target_port,
                    });
                }

                let (client, base_url, token) = daemon_client(daemon_alias.as_deref())?;
                let mut request = client
                    .post(format!("{base_url}/forwards/multi"))
                    .bearer_auth(token);
                if let Some(ref v) = via {
                    request = request.query(&[("via", v)]);
                }
                let result: agent2ssh::MultiForwardResult =
                    daemon_json(request.json(&serde_json::json!({
                        "host": &host,
                        "rules": parsed_rules.iter().map(|r| {
                            serde_json::json!({
                                "direction": match r.direction {
                                    ForwardDirection::Local => "local",
                                    ForwardDirection::Remote => "remote",
                                },
                                "bind_port": r.bind_port,
                                "target_host": &r.target_host,
                                "target_port": r.target_port,
                            })
                        }).collect::<Vec<_>>()
                    })))
                    .await?;

                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    println!("Started {} forward(s) on '{}':", result.count, result.host);
                    for id in &result.ids {
                        println!("  id={}", id);
                    }
                }
            }
            ForwardCommands::List { json } => {
                let (client, base_url, token) = daemon_client(daemon_alias.as_deref())?;
                let rules: Vec<ForwardRule> = daemon_json(
                    client
                        .get(format!("{base_url}/forwards"))
                        .bearer_auth(token),
                )
                .await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&rules)?);
                } else if rules.is_empty() {
                    println!("No active forwards.");
                } else {
                    for r in &rules {
                        println!(
                            "{}\t{}\t{}\t{}:{} → {}:{}",
                            r.id,
                            r.host,
                            r.direction,
                            r.bind_port,
                            r.target_host,
                            r.host,
                            r.target_port
                        );
                    }
                }
            }
            ForwardCommands::Rm { id, json } => {
                let (client, base_url, token) = daemon_client(daemon_alias.as_deref())?;
                let _: serde_json::Value = daemon_json(
                    client
                        .delete(format!("{base_url}/forwards/{id}"))
                        .bearer_auth(token),
                )
                .await?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({ "removed": id }))?
                    );
                } else {
                    println!("Forward {id} removed.");
                }
            }
        },
        Commands::Secrets { command } => match command {
            SecretsCommands::Status { json } => {
                let initialized = agent2ssh::secrets::is_initialized();
                let unlocked =
                    agent2ssh::secrets::is_unlocked() || agent2ssh::secrets::try_unlock_from_env();
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "initialized": initialized,
                            "unlocked": unlocked,
                        }))?
                    );
                } else {
                    println!(
                        "credential store: {} / {}",
                        if initialized {
                            "initialized"
                        } else {
                            "not set"
                        },
                        if unlocked { "unlocked" } else { "locked" }
                    );
                }
            }
            SecretsCommands::SetPassword { password } => {
                let password =
                    match password.or_else(|| std::env::var("AGENT2SSH_MASTER_PASSWORD").ok()) {
                        Some(p) if !p.is_empty() => p,
                        _ => {
                            eprint!("New master password: ");
                            use std::io::Write as _;
                            std::io::stderr().flush().ok();
                            let mut line = String::new();
                            std::io::stdin().read_line(&mut line)?;
                            line.trim_end_matches(['\n', '\r']).to_string()
                        }
                    };
                if password.is_empty() {
                    return Err(anyhow::anyhow!("master password must not be empty"));
                }
                let initialized = agent2ssh::secrets::is_initialized();
                if initialized {
                    // Changing requires the store to be unlocked first.
                    if !agent2ssh::secrets::is_unlocked()
                        && !agent2ssh::secrets::try_unlock_from_env()
                    {
                        return Err(anyhow::anyhow!(
                            "store is locked; set AGENT2SSH_MASTER_PASSWORD to the current password before changing it"
                        ));
                    }
                    agent2ssh::secrets::change_master_password(&password)?;
                    println!("Master password changed; credentials re-encrypted.");
                } else {
                    agent2ssh::secrets::unlock_or_init(&password)?;
                    let migrated = agent2ssh::migrate_plaintext_secrets().unwrap_or(0);
                    println!(
                        "Master password set.{}",
                        if migrated > 0 {
                            format!(" Encrypted {migrated} existing plaintext credential(s).")
                        } else {
                            String::new()
                        }
                    );
                }
            }
        },
        Commands::Ping {
            hosts,
            timeout_secs,
            json,
        } => {
            let results = ping_hosts_core(hosts, Some(timeout_secs)).await;
            if json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else {
                for r in &results {
                    if r.reachable {
                        println!("✓ {} ({}ms)", r.host, r.latency_ms.unwrap_or(0));
                    } else {
                        println!(
                            "✗ {}  {}",
                            r.host,
                            r.error.as_deref().unwrap_or("unreachable")
                        );
                    }
                }
            }
        }
        Commands::Audit {
            limit,
            json,
            host,
            risk,
            exit_code,
            since,
            until,
            search,
            command_pattern,
            env,
            role,
            owner,
            format,
            output,
        } => {
            let filter = AuditFilter {
                host,
                risk_level: risk,
                exit_code,
                since,
                until,
                limit,
                search,
                command_pattern,
                host_env: env,
                host_role: role,
                host_owner: owner,
            };

            // Handle --format export mode
            if let Some(ref fmt) = format {
                let exported = match fmt.to_lowercase().as_str() {
                    "jsonl" => export_audit_jsonl(&filter)?,
                    "csv" => export_audit_csv(&filter)?,
                    other => {
                        anyhow::bail!(
                            "unsupported export format '{}', expected 'jsonl' or 'csv'",
                            other
                        );
                    }
                };
                if let Some(ref path) = output {
                    std::fs::write(path, &exported)
                        .with_context(|| format!("failed to write to '{}'", path))?;
                    println!("Exported audit log to '{}' ({} format).", path, fmt);
                } else {
                    print!("{}", exported);
                }
                return Ok(());
            }

            let entries = list_audit_core(filter)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                for entry in entries {
                    println!(
                        "{}\t{}\texit={:?}\t{}ms\t[{}]\t{}",
                        entry.ts,
                        entry.host,
                        entry.exit_code,
                        entry.duration_ms,
                        entry.risk_level,
                        entry.command
                    );
                }
            }
        }
        Commands::Risk {
            command,
            host,
            json,
        } => {
            let risk = if let Some(host) = host.as_deref() {
                let target = command_authorization_target(host);
                agent2ssh::core::apply_risk_override(
                    effective_command_risk(&command).await,
                    target.risk_override,
                )
            } else {
                effective_command_risk(&command).await
            };
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "command": command,
                        "risk_level": risk,
                    }))?
                );
            } else {
                println!("Command: {}", command);
                println!("Risk:    {}", risk);
            }
        }
        Commands::Daemon { command } => {
            let config_dir = agent2ssh::config_dir()?;
            match command {
                DaemonCommands::Start => {
                    if let Some(pid) = read_daemon_pid()? {
                        if daemon_process_is_alive(pid) {
                            println!("Daemon is already running (pid={})", pid);
                            return Ok(());
                        }
                        remove_daemon_pid_file();
                    }
                    let daemon_bin = daemon_binary_path()?;
                    let started = start_daemon_background(&daemon_bin)?;
                    println!(
                        "Daemon started (pid={}). Log: {}",
                        started.pid,
                        started.log_path.display()
                    );
                }
                DaemonCommands::Stop => {
                    let Some(pid) = read_daemon_pid()? else {
                        println!("Daemon is not running (no PID file).");
                        return Ok(());
                    };
                    terminate_process(pid)?;
                    remove_daemon_pid_file();
                    println!("Daemon stopped (pid={}).", pid);
                }
                DaemonCommands::Status => {
                    let Some(pid) = read_daemon_pid()? else {
                        println!("Daemon is not running.");
                        return Ok(());
                    };
                    if !daemon_process_is_alive(pid) {
                        println!(
                            "Daemon PID file exists (pid={}) but process is not running.",
                            pid
                        );
                        return Ok(());
                    }
                    match daemon_health_body().await {
                        Ok(body) => {
                            println!("Daemon is running (pid={}).", pid);
                            println!("{body}");
                        }
                        Err(_) => {
                            println!(
                                "Daemon PID file exists (pid={}) but health check failed.",
                                pid
                            );
                        }
                    }
                }
                DaemonCommands::Restart => {
                    if let Some(pid) = read_daemon_pid()? {
                        terminate_process(pid)?;
                        remove_daemon_pid_file();
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                    let daemon_bin = daemon_binary_path()?;
                    let started = start_daemon_background(&daemon_bin)?;
                    println!(
                        "Daemon restarted (pid={}). Log: {}",
                        started.pid,
                        started.log_path.display()
                    );
                }
                DaemonCommands::RotateToken { json } => {
                    if let Some(pid) = read_daemon_pid()? {
                        if daemon_process_is_alive(pid) {
                            anyhow::bail!(
                                "daemon is running (pid={pid}); stop it before rotating daemon.token"
                            );
                        }
                        remove_daemon_pid_file();
                    }
                    std::fs::create_dir_all(&config_dir)?;
                    let token_path = config_dir.join("daemon.token");
                    std::fs::write(&token_path, uuid::Uuid::new_v4().to_string())?;
                    restrict_file_to_owner(&token_path)?;
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "rotated": true,
                                "path": token_path,
                            }))?
                        );
                    } else {
                        println!("Rotated daemon token at {}.", token_path.display());
                        println!("Restart the daemon and update clients that use this token.");
                    }
                }
                DaemonCommands::List { json } => {
                    let daemons = list_daemons_core()?;
                    if json {
                        // Augment with version compatibility info
                        let mut augmented = Vec::new();
                        for d in &daemons {
                            let compat = if d.connected && d.alias != "localhost" {
                                check_daemon_version(&d.alias).await.ok().map(|c| {
                                    serde_json::json!({
                                        "local_version": c.local_version,
                                        "remote_version": c.remote_version,
                                        "compatible": c.compatible,
                                        "message": c.message,
                                    })
                                })
                            } else {
                                None
                            };
                            let mut info = serde_json::json!({
                                "alias": d.alias,
                                "url": d.url,
                                "connected": d.connected,
                            });
                            if let Some(c) = compat {
                                info["version_compatibility"] = c;
                            }
                            if let Some(ref scope) = d.scope {
                                info["scope"] = serde_json::to_value(scope).unwrap_or_default();
                            }
                            augmented.push(info);
                        }
                        println!("{}", serde_json::to_string_pretty(&augmented)?);
                    } else if daemons.is_empty() {
                        println!("No daemons configured.");
                    } else {
                        for d in &daemons {
                            let status = if d.connected {
                                "connected"
                            } else {
                                "unreachable"
                            };
                            let mut line = format!("{}\t{}\t[{}]", d.alias, d.url, status);
                            if d.connected && d.alias != "localhost" {
                                if let Ok(compat) = check_daemon_version(&d.alias).await {
                                    if !compat.compatible {
                                        line.push_str(&format!(
                                            "\t[version: incompatible - {}]",
                                            compat.message
                                        ));
                                    } else if compat.remote_version.as_deref()
                                        != Some(PROTOCOL_VERSION)
                                    {
                                        line.push_str(&format!(
                                            "\t[version: {}]",
                                            compat.remote_version.as_deref().unwrap_or("?")
                                        ));
                                    }
                                }
                            }
                            if let Some(ref scope) = d.scope {
                                let mut scope_parts = Vec::new();
                                if !scope.allowed_hosts.is_empty() {
                                    scope_parts
                                        .push(format!("hosts={}", scope.allowed_hosts.len()));
                                }
                                if !scope.allowed_tags.is_empty() {
                                    scope_parts.push(format!("tags={}", scope.allowed_tags.len()));
                                }
                                if !scope.allowed_commands.is_empty() {
                                    scope_parts
                                        .push(format!("cmds={}", scope.allowed_commands.len()));
                                }
                                if !scope.denied_commands.is_empty() {
                                    scope_parts
                                        .push(format!("denied={}", scope.denied_commands.len()));
                                }
                                if !scope_parts.is_empty() {
                                    line.push_str(&format!(
                                        "\t[scope: {}]",
                                        scope_parts.join(", ")
                                    ));
                                } else {
                                    line.push_str("\t[scope: open]");
                                }
                            }
                            println!("{}", line);
                        }
                    }
                }
                DaemonCommands::View { json } => {
                    let view = get_daemons_unified_view().await?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&view)?);
                    } else {
                        println!("Daemon Unified View");
                        println!("{:-<80}", "");
                        println!(
                            "{:<15} {:<12} {:<10} {:<8} {:<10}",
                            "ALIAS", "STATUS", "VERSION", "HOSTS", "EXECS"
                        );
                        for d in &view.daemons {
                            let status = if d.connected { "connected" } else { "offline" };
                            let version = d
                                .health
                                .as_ref()
                                .and_then(|h| h.version.as_deref())
                                .unwrap_or("-");
                            let hosts = d
                                .host_count
                                .map(|c| c.to_string())
                                .unwrap_or_else(|| "-".into());
                            let execs = d
                                .metrics
                                .as_ref()
                                .and_then(|m| m.exec_count)
                                .map(|c| c.to_string())
                                .unwrap_or_else(|| "-".into());
                            println!(
                                "{:<15} {:<12} {:<10} {:<8} {:<10}",
                                d.alias, status, version, hosts, execs
                            );
                        }
                        println!("{:-<80}", "");
                        println!(
                            "Total: {} daemon(s), {} connected, {} hosts",
                            view.daemons.len(),
                            view.total_connected,
                            view.total_hosts
                        );
                    }
                }
            }
        }
        Commands::Pause { json, reason } => {
            let status = update_gate(daemon_alias.as_deref(), "pause", reason).await?;
            print_gate_status(&status, json)?;
        }
        Commands::Resume { json, reason } => {
            let status = update_gate(daemon_alias.as_deref(), "resume", reason).await?;
            print_gate_status(&status, json)?;
        }
        Commands::Status { json } => {
            let (client, base_url, token) = daemon_client(daemon_alias.as_deref())?;
            let status: ExecutionGateStatus =
                daemon_json(client.get(format!("{base_url}/gate")).bearer_auth(token)).await?;
            print_gate_status(&status, json)?;
        }
        Commands::ConfigExport { json } => {
            let export = export_team_config()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&export)?);
            } else {
                println!("Team config export:");
                println!("  Hosts: {} (key paths stripped)", export.hosts.len());
                for h in &export.hosts {
                    println!(
                        "    {}\t{}{}:{}",
                        h.name,
                        h.user
                            .as_deref()
                            .map(|u| format!("{u}@"))
                            .unwrap_or_default(),
                        h.host,
                        h.port.unwrap_or(22)
                    );
                }
                println!(
                    "  Risk rules: {}",
                    if export.risk_rules.is_some() {
                        "included"
                    } else {
                        "not configured"
                    }
                );
                println!(
                    "  Playbooks:  {}",
                    if export.playbooks.is_some() {
                        "included"
                    } else {
                        "not configured"
                    }
                );
                if json {
                    println!("\nUse --json to output machine-readable JSON.");
                }
            }
        }
        Commands::ConfigImport {
            path,
            json,
            preview,
        } => {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| anyhow::anyhow!("failed to read '{}': {}", path, e))?;
            let export: TeamConfigExport = serde_json::from_str(&raw)
                .map_err(|e| anyhow::anyhow!("failed to parse JSON from '{}': {}", path, e))?;

            if preview {
                let diff = agent2ssh::preview_team_config_import(&export)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&diff)?);
                } else {
                    println!("Config Import Preview:");
                    println!("  {}", diff.summary);
                    if !diff.hosts_to_add.is_empty() {
                        println!("  Hosts to add:");
                        for h in &diff.hosts_to_add {
                            println!("    + {}", h);
                        }
                    }
                    if !diff.hosts_to_skip.is_empty() {
                        println!("  Hosts to skip (duplicates):");
                        for h in &diff.hosts_to_skip {
                            println!("    ~ {}", h);
                        }
                    }
                    if !diff.hosts_to_update.is_empty() {
                        println!("  Hosts to update:");
                        for h in &diff.hosts_to_update {
                            println!("    * {}", h);
                        }
                    }
                }
                return Ok(());
            }

            let result = import_team_config(&export)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Import complete:");
                println!("  Hosts added:   {}", result.hosts_added);
                println!("  Hosts updated: {}", result.hosts_updated);
                println!("  Hosts skipped: {}", result.hosts_skipped);
                println!(
                    "  Risk rules:    {}",
                    if result.risk_rules_imported {
                        "imported"
                    } else {
                        "not included"
                    }
                );
                println!(
                    "  Playbooks:     {}",
                    if result.playbooks_imported {
                        "imported"
                    } else {
                        "not included"
                    }
                );
            }
        }
        Commands::SshSync {
            diff,
            export,
            path,
            json,
        } => {
            let ssh_path = path.as_deref();
            if diff || !export {
                // Default: show diff
                let result = agent2ssh::compare_ssh_configs(ssh_path)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    println!("SSH Config Sync: {}", result.summary);
                    if !result.only_in_agent2ssh.is_empty() {
                        println!("\n  Only in Agent2SSH:");
                        for h in &result.only_in_agent2ssh {
                            println!("    + {} ({})", h.name, h.host);
                        }
                    }
                    if !result.only_in_ssh_config.is_empty() {
                        println!("\n  Only in ~/.ssh/config:");
                        for h in &result.only_in_ssh_config {
                            println!("    + {} ({})", h.name, h.host);
                        }
                    }
                    if !result.conflicts.is_empty() {
                        println!("\n  Conflicts:");
                        for c in &result.conflicts {
                            println!(
                                "    {} {}: '{}' (agent2ssh) vs '{}' (ssh config)",
                                c.name, c.field, c.agent2ssh_value, c.ssh_config_value
                            );
                        }
                    }
                    if !result.matching.is_empty() {
                        println!("\n  Matching: {}", result.matching.join(", "));
                    }
                }
            }
            if export {
                let (out_path, count) = agent2ssh::export_to_ssh_config(ssh_path, None)?;
                if json {
                    println!(
                        "{}",
                        serde_json::json!({ "path": out_path, "hosts_exported": count })
                    );
                } else {
                    println!("Exported {} hosts to {}", count, out_path);
                }
            }
        }
        Commands::Webdav { command } => match command {
            WebDavCommands::Push { options } => {
                let json = options.json;
                let result = agent2ssh::webdav_push(options.into_sync_options()).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    print_webdav_result(&result);
                }
            }
            WebDavCommands::Pull { options } => {
                let json = options.json;
                let result = agent2ssh::webdav_pull(options.into_sync_options()).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    print_webdav_result(&result);
                }
            }
            WebDavCommands::Status { options } => {
                let json = options.json;
                let status = agent2ssh::webdav_status(options.into_sync_options()).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&status)?);
                } else {
                    print_webdav_status(&status);
                }
            }
        },
        Commands::Doctor {
            json: output_json,
            daemon,
        } => {
            if let Some(ref alias) = daemon {
                run_daemon_doctor(alias, output_json).await?;
            } else {
                run_doctor(output_json).await?;
            }
        }
        Commands::Health { json, hosts } => {
            let target_hosts = match hosts {
                Some(h) if !h.is_empty() => h,
                _ => {
                    // Collect health for ALL configured hosts
                    let config =
                        agent2ssh::store::load_config().context("failed to load configuration")?;
                    config.hosts.iter().map(|h| h.name.clone()).collect()
                }
            };
            let snapshot = collect_health_snapshot(target_hosts, None).await;
            if json {
                println!("{}", serde_json::to_string_pretty(&snapshot)?);
            } else {
                println!("Health Snapshot (collected in {}ms):", snapshot.total_ms);
                println!("{:-<70}", "");
                for h in &snapshot.hosts {
                    let status = if h.reachable { "UP" } else { "DOWN" };
                    let latency = h
                        .latency_ms
                        .map(|ms| format!("{ms}ms"))
                        .unwrap_or_else(|| "-".into());
                    println!(
                        "[{}] {}\tlatency={}\t{}",
                        status,
                        h.host,
                        latency,
                        h.error.as_deref().unwrap_or("")
                    );
                    if let Some(ref uptime) = h.uptime {
                        println!("  uptime:   {}", uptime.trim());
                    }
                    if let Some(ref load) = h.load_avg {
                        println!("  load avg: {}", load);
                    }
                    if let Some(ref disk) = h.disk_usage {
                        println!("  disk:     {}", disk.trim());
                    }
                    if let Some(ref mem) = h.memory_usage {
                        for line in mem.lines() {
                            println!("  mem:      {}", line.trim());
                        }
                    }
                }
            }
        }
        Commands::Policy { command } => match command {
            PolicyCommands::Validate { path, json } => {
                let policy = validate_policy_path(path.as_deref())?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "valid": true,
                            "risk_rules": {
                                "blocked": policy.risk.blocked.patterns.len(),
                                "high": policy.risk.high.patterns.len(),
                                "medium": policy.risk.medium.patterns.len(),
                            },
                            "approval_policies": policy.approval.policies.len(),
                        }))?
                    );
                } else {
                    println!(
                        "Policy valid: {} blocked, {} high, {} medium risk rules; {} approval policies.",
                        policy.risk.blocked.patterns.len(),
                        policy.risk.high.patterns.len(),
                        policy.risk.medium.patterns.len(),
                        policy.approval.policies.len()
                    );
                }
            }
            PolicyCommands::Test {
                command,
                host,
                json,
            } => {
                let result = test_policy_decision(&host, &command).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    let approval = result
                        .matched_approval_policy
                        .as_deref()
                        .map(|name| format!(", policy={name}"))
                        .unwrap_or_default();
                    let user_rule = if result.matched_user_rule {
                        ", user_rule=true"
                    } else {
                        ""
                    };
                    println!(
                        "{}\trisk={}{}{}",
                        result.decision, result.risk_level, approval, user_rule
                    );
                }
            }
            PolicyCommands::List { json } => {
                let policies = list_approval_policies()?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&policies)?);
                } else if policies.is_empty() {
                    println!("No approval policies configured.");
                } else {
                    for p in &policies {
                        let hosts = if p.hosts.is_empty() {
                            "*".to_string()
                        } else {
                            p.hosts.join(", ")
                        };
                        let tags = if p.tags.is_empty() {
                            "*".to_string()
                        } else {
                            p.tags.join(", ")
                        };
                        let risk = p
                            .min_risk
                            .map(|r| r.to_string())
                            .unwrap_or_else(|| "any".to_string());
                        let pattern = p.command_pattern.as_deref().unwrap_or("*");
                        let action = if p.requires_approval {
                            "require"
                        } else {
                            "auto-approve"
                        };
                        let ttl = p
                            .ttl_secs
                            .map(|t| format!("{t}s"))
                            .unwrap_or_else(|| "default".to_string());
                        println!(
                            "{}\thosts={}\ttags={}\tmin_risk={}\tpattern={}\t{}\tTTL={}",
                            p.name, hosts, tags, risk, pattern, action, ttl
                        );
                    }
                }
            }
            PolicyCommands::Add {
                name,
                hosts,
                tags,
                min_risk,
                command_pattern,
                auto_approve,
                ttl_secs,
                json,
            } => {
                let mut policies = load_approval_policies()?;
                let min_risk_level = min_risk.and_then(|s| match s.to_lowercase().as_str() {
                    "low" => Some(RiskLevel::Low),
                    "medium" => Some(RiskLevel::Medium),
                    "high" => Some(RiskLevel::High),
                    "blocked" => Some(RiskLevel::Blocked),
                    _ => None,
                });
                let policy = ApprovalPolicy {
                    name: name.clone(),
                    hosts: hosts.unwrap_or_default(),
                    tags: tags.unwrap_or_default(),
                    min_risk: min_risk_level,
                    command_pattern,
                    requires_approval: !auto_approve,
                    ttl_secs,
                };
                policies.push(policy);
                save_approval_policies(&policies)?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(
                            &serde_json::json!({ "added": name, "total": policies.len() })
                        )?
                    );
                } else {
                    println!("Policy '{}' added ({} total).", name, policies.len());
                }
            }
            PolicyCommands::Remove { name, json } => {
                let mut policies = load_approval_policies()?;
                let before = policies.len();
                policies.retain(|p| p.name != name);
                if policies.len() == before {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(
                                &serde_json::json!({ "removed": false, "name": name })
                            )?
                        );
                    } else {
                        println!("Policy '{}' not found.", name);
                    }
                } else {
                    save_approval_policies(&policies)?;
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(
                                &serde_json::json!({ "removed": true, "name": name, "remaining": policies.len() })
                            )?
                        );
                    } else {
                        println!("Policy '{}' removed ({} remaining).", name, policies.len());
                    }
                }
            }
            PolicyCommands::Check {
                host,
                command,
                json,
            } => {
                let target = command_authorization_target(&host);
                let (risk, _matched_user_rule) =
                    effective_risk_for_policy(&command, target.risk_override).await;
                let result = check_approval_required(&host, &target.tags, &command, risk)?;
                if json {
                    let output = match &result {
                        Some(policy) => serde_json::json!({
                            "requires_approval": true,
                            "matched_policy": policy.name,
                            "risk_level": risk,
                            "ttl_secs": policy.ttl_secs,
                        }),
                        None => serde_json::json!({
                            "requires_approval": false,
                            "risk_level": risk,
                        }),
                    };
                    println!("{}", serde_json::to_string_pretty(&output)?);
                } else {
                    match &result {
                        Some(policy) => {
                            println!(
                                "Approval REQUIRED for '{}' on '{}' (risk: {}, policy: '{}')",
                                command, host, risk, policy.name
                            );
                        }
                        None => {
                            println!(
                                "No approval needed for '{}' on '{}' (risk: {})",
                                command, host, risk
                            );
                        }
                    }
                }
            }
        },
        Commands::Snippet { command } => match command {
            SnippetCommands::List { json } => {
                let snippets = if let Some(alias) = daemon_alias.as_deref() {
                    if alias != "localhost" {
                        remote_snippet_list(alias).await?
                    } else {
                        load_snippets()?
                    }
                } else {
                    load_snippets()?
                };
                if json {
                    println!("{}", serde_json::to_string_pretty(&snippets)?);
                } else if snippets.is_empty() {
                    println!("No snippets configured.");
                } else {
                    for snippet in snippets {
                        let description = snippet.description.as_deref().unwrap_or("");
                        println!("{}\t{}\t{}", snippet.name, snippet.command, description);
                    }
                }
            }
            SnippetCommands::Save {
                name,
                command,
                description,
                json,
            } => {
                let snippet = Snippet {
                    name,
                    command,
                    description,
                };
                let snippets = if let Some(alias) = daemon_alias.as_deref() {
                    if alias != "localhost" {
                        remote_snippet_save(alias, &snippet).await?
                    } else {
                        add_snippet(
                            &snippet.name,
                            &snippet.command,
                            snippet.description.as_deref(),
                        )?
                    }
                } else {
                    add_snippet(
                        &snippet.name,
                        &snippet.command,
                        snippet.description.as_deref(),
                    )?
                };
                if json {
                    println!("{}", serde_json::to_string_pretty(&snippets)?);
                } else {
                    println!("Saved snippet '{}'.", snippet.name.trim());
                }
            }
            SnippetCommands::Delete { name, json } => {
                let removed = if let Some(alias) = daemon_alias.as_deref() {
                    if alias != "localhost" {
                        remote_snippet_delete(alias, &name).await?
                    } else {
                        remove_snippet(&name)?
                    }
                } else {
                    remove_snippet(&name)?
                };
                if json {
                    println!(
                        "{}",
                        serde_json::json!({ "removed": removed, "name": name })
                    );
                } else if removed {
                    println!("Deleted snippet '{}'.", name.trim());
                } else {
                    println!("Snippet '{}' was not found.", name.trim());
                }
            }
        },
        Commands::Playbook { command } => match command {
            PlaybookCommands::List { json } => {
                let playbooks = agent2ssh::list_playbooks_core()?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&playbooks)?);
                } else if playbooks.is_empty() {
                    println!("No playbooks configured.");
                } else {
                    for pb in &playbooks {
                        let step_count = pb
                            .advanced_steps
                            .as_ref()
                            .map(|a| a.len())
                            .unwrap_or(pb.steps.len());
                        println!("{}\t{} step(s)\t{}", pb.name, step_count, pb.description);
                    }
                }
            }
            PlaybookCommands::Run {
                name,
                host,
                force,
                params,
                reason,
                change_id,
                json,
            } => {
                let params_map = parse_cli_params(params);
                let source = source_from_transport();
                let approved_steps = authorize_local_playbook_run(
                    &name,
                    &host,
                    force,
                    &params_map,
                    reason.clone(),
                    change_id.clone(),
                    &source,
                )
                .await?;
                let result = run_playbook_core_with_source_and_approved_steps(
                    &name,
                    &host,
                    force,
                    if params_map.is_empty() {
                        None
                    } else {
                        Some(&params_map)
                    },
                    reason,
                    change_id,
                    Some(source),
                    &approved_steps,
                )
                .await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else if result.success {
                    println!(
                        "Playbook '{}' completed on '{}' ({} step(s), {}ms).",
                        result.playbook,
                        result.host,
                        result.steps_completed.len(),
                        result.total_duration_ms
                    );
                } else {
                    eprintln!(
                        "Playbook '{}' failed on '{}' after {} step(s) ({}ms).",
                        result.playbook,
                        result.host,
                        result.steps_completed.len(),
                        result.total_duration_ms
                    );
                    if let Some(last) = result.steps_completed.last() {
                        if let Some(ref err) = last.error {
                            eprintln!("Error at step {}: {}", last.step, err);
                        }
                    }
                    std::process::exit(1);
                }
            }
            PlaybookCommands::DryRun { name, params } => {
                let params_map = parse_cli_params(params);
                let dry_run = dry_run_playbook(&name, &params_map)?;
                println!("Playbook: {}", dry_run.playbook);
                println!("Steps:");
                for step in &dry_run.steps {
                    println!("  [{}] Template: {}", step.step, step.command_template);
                    println!("         Resolved: {}", step.command_resolved);
                    if !step.params_used.is_empty() {
                        println!("         Params:   {}", step.params_used.join(", "));
                    }
                }
            }
        },
        Commands::Metrics { command } => match command {
            MetricsCommands::Trend { period, json } => {
                let trend_period = match period.to_lowercase().as_str() {
                    "24h" | "last24h" => TrendPeriod::Last24h,
                    "7d" | "last7d" => TrendPeriod::Last7d,
                    "30d" | "last30d" => TrendPeriod::Last30d,
                    "all" => TrendPeriod::All,
                    other => {
                        anyhow::bail!("unknown period '{}'. Use: 24h, 7d, 30d, or all", other);
                    }
                };
                let trend = compute_metrics_trend(trend_period)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&trend)?);
                } else {
                    println!("Metrics Trend ({})", period);
                    println!("{:-<60}", "");
                    println!("Total executions : {}", trend.total_executions);
                    println!("Successful       : {}", trend.success_count);
                    println!("Failed           : {}", trend.failure_count);
                    println!("Blocked          : {}", trend.blocked_count);
                    println!("Failure rate     : {:.1}%", trend.failure_rate * 100.0);
                    println!("Avg duration     : {:.1}ms", trend.avg_duration_ms);
                    println!();
                    println!("Risk distribution:");
                    println!("  low     : {}", trend.risk_distribution.low);
                    println!("  medium  : {}", trend.risk_distribution.medium);
                    println!("  high    : {}", trend.risk_distribution.high);
                    println!("  blocked : {}", trend.risk_distribution.blocked);
                    if !trend.top_hosts.is_empty() {
                        println!();
                        println!("Top hosts:");
                        for h in &trend.top_hosts {
                            println!("  {:<20} {} executions", h.host, h.count);
                        }
                    }
                }
            }
        },
        Commands::Events { json } => {
            let mut rx = subscribe_events();
            println!("Subscribed to event stream (Ctrl+C to exit)...");
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if json {
                            println!("{}", serde_json::to_string(&event)?);
                        } else {
                            println!(
                                "[{}] {} {}",
                                event.timestamp.format("%H:%M:%S"),
                                serde_json::to_string(&event.event_type)?,
                                serde_json::to_string(&event.data)?,
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("Event stream error: {}", e);
                        break;
                    }
                }
            }
        }
        Commands::Integrate { command } => run_integrate_command(command)?,
        Commands::VersionCheck { repo, json } => {
            let repo = repo.as_deref().unwrap_or("lengyuqu/agent2ssh");
            // Validate repo format
            let repo_parts: Vec<&str> = repo.split('/').collect();
            if repo_parts.len() != 2
                || repo_parts.iter().any(|part| part.is_empty())
                || repo.chars().any(|c| c.is_whitespace())
            {
                return Err(anyhow::anyhow!(
                    "invalid repo format '{}': expected 'owner/name'",
                    repo
                ));
            }
            let local_version = env!("CARGO_PKG_VERSION");
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(format!("agent2ssh/{local_version}"))
                .timeout(std::time::Duration::from_secs(10))
                .build()?;
            let url = format!("https://github.com/{}/releases/latest", repo);
            let resp = client.get(&url).send().await?;
            if !resp.status().is_redirection() {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "error": format!("expected redirect from GitHub, got status {}", resp.status()),
                            "url": url,
                        })
                    );
                } else {
                    eprintln!(
                        "error: GitHub returned status {} (expected redirect)",
                        resp.status()
                    );
                }
                return Ok(());
            }
            let location = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let latest_tag = location
                .rsplit("/releases/tag/")
                .next()
                .unwrap_or("")
                .trim_end_matches('/');
            if latest_tag.is_empty() {
                return Err(anyhow::anyhow!(
                    "could not parse release tag from redirect: {}",
                    location
                ));
            }
            let normalized_tag = latest_tag.trim_start_matches(['v', 'V']);
            let compat = agent2ssh::remote::check_version_compatibility(Some(normalized_tag));
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "local_version": local_version,
                        "latest_version": latest_tag,
                        "compatible": compat.compatible,
                        "message": compat.message,
                        "repo": repo,
                    }))?
                );
            } else {
                println!("Local version:  {}", local_version);
                println!("Latest version: {}", latest_tag);
                println!("Status:         {}", compat.message);
                if latest_tag.trim_start_matches('v') != local_version {
                    println!(
                        "URL:            https://github.com/{}/releases/tag/{}",
                        repo, latest_tag
                    );
                }
            }
        }
    }

    Ok(())
}

// ── Integrate command ────────────────────────────────────────────────────────

fn integrate_skill_dir(dir: Option<PathBuf>) -> Result<PathBuf> {
    match dir {
        Some(dir) => Ok(dir),
        None => agent2ssh::integrate::default_skill_dir(),
    }
}

fn print_skill_status(status: &agent2ssh::integrate::AgentSkillStatus, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(status)?);
        return Ok(());
    }
    println!("Skill directory: {}", status.dir);
    if status.installed {
        println!(
            "Installed:       yes (version {})",
            status.installed_version.as_deref().unwrap_or("unknown")
        );
    } else {
        println!("Installed:       no");
    }
    println!(
        "Bundled version: {}",
        status.available_version.as_deref().unwrap_or("unknown")
    );
    if status.update_available {
        println!("Update available — run `agent2ssh integrate skill install` to update.");
    }
    Ok(())
}

fn run_integrate_command(command: IntegrateCommands) -> Result<()> {
    match command {
        IntegrateCommands::List { json } => {
            let statuses = agent2ssh::integrate::list_mcp_client_configs()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&statuses)?);
            } else {
                println!("{:<16} {:<16} {:<14} CONFIG", "CLIENT", "NAME", "STATUS");
                println!("{:-<70}", "");
                for status in &statuses {
                    println!(
                        "{:<16} {:<16} {:<14} {}",
                        status.id, status.name, status.status, status.config_path
                    );
                }
                println!();
                println!("Register a client:   agent2ssh integrate add <client>");
                println!("Install the skill:   agent2ssh integrate skill install");
            }
        }
        IntegrateCommands::Add { client, json } => {
            let result = agent2ssh::integrate::configure_mcp_client(&client)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", result.message);
                println!("Config:  {}", result.config_path);
                if let Some(backup) = &result.backup_path {
                    println!("Backup:  {}", backup);
                }
                println!("Command: {}", result.command);
            }
        }
        IntegrateCommands::Rm { client, json } => {
            let result = agent2ssh::integrate::uninstall_mcp_client(&client)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", result.message);
                if let Some(backup) = &result.backup_path {
                    println!("Backup: {}", backup);
                }
            }
        }
        IntegrateCommands::Skill { command } => match command {
            IntegrateSkillCommands::Status { dir, json } => {
                let dir = integrate_skill_dir(dir)?;
                let status = agent2ssh::integrate::agent_skill_status_at(&dir);
                print_skill_status(&status, json)?;
            }
            IntegrateSkillCommands::Install { dir, json } => {
                let dir = integrate_skill_dir(dir)?;
                let status = agent2ssh::integrate::install_agent_skill_at(&dir)?;
                if !json {
                    println!("Skill installed to {}", status.path);
                }
                print_skill_status(&status, json)?;
            }
            IntegrateSkillCommands::Uninstall { dir, json } => {
                let dir = integrate_skill_dir(dir)?;
                let removed = agent2ssh::integrate::uninstall_agent_skill_at(&dir)?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "dir": dir.display().to_string(),
                            "removed": removed,
                        }))?
                    );
                } else if removed {
                    println!("Skill removed from {}", dir.display());
                } else {
                    println!("No skill installed at {}", dir.display());
                }
            }
        },
    }
    Ok(())
}

fn parse_cli_params(params: Option<Vec<String>>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(items) = params {
        for item in items {
            if let Some(pos) = item.find('=') {
                let key = item[..pos].to_string();
                let value = item[pos + 1..].to_string();
                map.insert(key, value);
            }
        }
    }
    map
}

fn daemon_binary_path() -> Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    let daemon_dir = exe
        .parent()
        .context("failed to resolve current executable directory")?;
    #[cfg(windows)]
    let daemon_name = "agent2ssh-daemon.exe";
    #[cfg(not(windows))]
    let daemon_name = "agent2ssh-daemon";
    Ok(daemon_dir.join(daemon_name))
}

// ── Doctor command ───────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct DoctorCheck {
    name: String,
    status: String, // "pass", "fail", "warn"
    detail: String,
}

async fn run_doctor(output_json: bool) -> Result<()> {
    let mut checks: Vec<DoctorCheck> = Vec::new();

    checks.push(DoctorCheck {
        name: "embedded SSH transport".into(),
        status: "pass".into(),
        detail: "exec, SFTP, terminal, sessions, jump hosts, connections, and forwards use the Rust backend".into(),
    });

    checks.push(DoctorCheck {
        name: "embedded key generation".into(),
        status: "pass".into(),
        detail: "Ed25519 keys are generated with the Rust backend and system CSPRNG".into(),
    });

    // 3. Check ~/.agent2ssh/ directory exists and is writable
    let config_dir = agent2ssh::config_dir()?;
    let dir_exists = config_dir.exists();
    let dir_writable = if dir_exists {
        // Try to create a temp file to verify write access
        let probe = config_dir.join(".doctor_probe");
        match std::fs::write(&probe, "ok") {
            Ok(_) => {
                let _ = std::fs::remove_file(&probe);
                true
            }
            Err(_) => false,
        }
    } else {
        false
    };
    checks.push(DoctorCheck {
        name: "config directory".into(),
        status: if dir_exists && dir_writable {
            "pass"
        } else if dir_exists {
            "warn"
        } else {
            "fail"
        }
        .into(),
        detail: format!(
            "{} ({})",
            config_dir.display(),
            if dir_exists && dir_writable {
                "exists, writable"
            } else if dir_exists {
                "exists, NOT writable"
            } else {
                "does not exist"
            }
        ),
    });

    // 4. Check hosts.json exists and is valid JSON
    let hosts_path = config_dir.join("hosts.json");
    if hosts_path.exists() {
        let hosts_valid = std::fs::read_to_string(&hosts_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .is_some();
        checks.push(DoctorCheck {
            name: "hosts.json".into(),
            status: if hosts_valid { "pass" } else { "fail" }.into(),
            detail: if hosts_valid {
                "valid JSON".into()
            } else {
                "exists but invalid JSON".into()
            },
        });
    } else {
        checks.push(DoctorCheck {
            name: "hosts.json".into(),
            status: "warn".into(),
            detail: "not configured yet (no hosts.json)".into(),
        });
    }

    // 5. Check daemon.token exists and permissions are 0600 (Unix)
    let token_path = config_dir.join("daemon.token");
    if token_path.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&token_path)
                .map(|m| m.permissions().mode() & 0o777)
                .unwrap_or(0o777);
            let ok = mode == 0o600;
            checks.push(DoctorCheck {
                name: "daemon.token".into(),
                status: if ok { "pass" } else { "warn" }.into(),
                detail: if ok {
                    format!("permissions 0{:o} (correct)", mode)
                } else {
                    format!("permissions 0{:o} (should be 0600)", mode)
                },
            });
        }
        #[cfg(not(unix))]
        {
            checks.push(DoctorCheck {
                name: "daemon.token".into(),
                status: "pass".into(),
                detail: "exists".into(),
            });
        }
    } else {
        checks.push(DoctorCheck {
            name: "daemon.token".into(),
            status: "warn".into(),
            detail: "not found (daemon not started?)".into(),
        });
    }

    // 6. Check daemon is running (hit /health)
    let daemon_running = check_daemon_health().await;
    checks.push(DoctorCheck {
        name: "daemon health".into(),
        status: if daemon_running { "pass" } else { "warn" }.into(),
        detail: if daemon_running {
            "GET /health returned 200".into()
        } else {
            format!("daemon not reachable on {}", agent2ssh::local_daemon_url())
        },
    });

    // 7. Check optional config files
    let optional_files = [
        ("risk_rules.toml", "risk rules"),
        ("playbooks.toml", "playbooks"),
        ("remotes.toml", "remote daemons"),
        ("webhook.toml", "webhook config"),
    ];
    for (filename, label) in &optional_files {
        let path = config_dir.join(filename);
        let exists = path.exists();
        checks.push(DoctorCheck {
            name: format!("{filename} ({label})"),
            status: if exists { "pass" } else { "warn" }.into(),
            detail: if exists {
                "present".into()
            } else {
                format!("not found ({label} unavailable)")
            },
        });
    }

    // 8. Check audit log size
    if let Ok(audit_p) = audit_path() {
        if audit_p.exists() {
            let size = std::fs::metadata(&audit_p).map(|m| m.len()).unwrap_or(0);
            let size_mb = size as f64 / (1024.0 * 1024.0);
            let status = if size_mb > 10.0 { "warn" } else { "pass" };
            checks.push(DoctorCheck {
                name: "audit log".into(),
                status: status.into(),
                detail: format!("{:.2} MB ({})", size_mb, audit_p.display()),
            });
        } else {
            checks.push(DoctorCheck {
                name: "audit log".into(),
                status: "pass".into(),
                detail: "no audit log yet".into(),
            });
        }
    }

    // Output
    if output_json {
        println!("{}", serde_json::to_string_pretty(&checks)?);
    } else {
        println!("agent2ssh doctor report");
        println!("{:-<60}", "");
        for check in &checks {
            let icon = match check.status.as_str() {
                "pass" => "[PASS]",
                "fail" => "[FAIL]",
                "warn" => "[WARN]",
                _ => "[????]",
            };
            println!("{} {:<28} {}", icon, check.name, check.detail);
        }
        let fail_count = checks.iter().filter(|c| c.status == "fail").count();
        let warn_count = checks.iter().filter(|c| c.status == "warn").count();
        println!("{:-<60}", "");
        println!(
            "Summary: {} check(s), {} fail, {} warn",
            checks.len(),
            fail_count,
            warn_count
        );
        if fail_count > 0 {
            std::process::exit(1);
        }
    }

    Ok(())
}

// ── Daemon Doctor (F5-1) ────────────────────────────────────────────────────

async fn run_daemon_doctor(alias: &str, output_json: bool) -> Result<()> {
    let diagnostic = diagnose_daemon(alias).await?;

    if output_json {
        println!("{}", serde_json::to_string_pretty(&diagnostic)?);
    } else {
        println!("Daemon diagnostic: {}", diagnostic.alias);
        println!("URL: {}", diagnostic.url);
        println!("{:-<60}", "");
        for check in &diagnostic.checks {
            let icon = match check.status {
                agent2ssh::remote::DiagnosticStatus::Ok => "[PASS]",
                agent2ssh::remote::DiagnosticStatus::Warning => "[WARN]",
                agent2ssh::remote::DiagnosticStatus::Error => "[FAIL]",
            };
            println!("{} {:<24} {}", icon, check.name, check.message);
            if let Some(ref details) = check.details {
                println!("  {:<26} {}", "", details);
            }
        }
        println!("{:-<60}", "");
        let overall_icon = match diagnostic.overall_status {
            agent2ssh::remote::DiagnosticStatus::Ok => "OK",
            agent2ssh::remote::DiagnosticStatus::Warning => "WARNING",
            agent2ssh::remote::DiagnosticStatus::Error => "ERROR",
        };
        println!(
            "Overall: {} ({} check(s))",
            overall_icon,
            diagnostic.checks.len()
        );

        if diagnostic.overall_status == agent2ssh::remote::DiagnosticStatus::Error {
            std::process::exit(1);
        }
    }

    Ok(())
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

fn print_host_row(host: &HostProfile) {
    let target = format!(
        "{}{}:{}",
        host.user
            .as_ref()
            .map(|u| format!("{u}@"))
            .unwrap_or_default(),
        host.host,
        host.port.unwrap_or(22)
    );
    let mut metadata = Vec::new();
    if let Some(env) = &host.env {
        metadata.push(format!("env={env}"));
    }
    if let Some(role) = &host.role {
        metadata.push(format!("role={role}"));
    }
    if let Some(owner) = &host.owner {
        metadata.push(format!("owner={owner}"));
    }
    if !host.tags.is_empty() {
        metadata.push(format!("tags={}", host.tags.join(",")));
    }

    if metadata.is_empty() {
        println!("{}\t{}", host.name, target);
    } else {
        println!("{}\t{}\t{}", host.name, target, metadata.join(" "));
    }
}

fn print_exec_plan(plan: &agent2ssh::ExecPlan) {
    println!("Execution Plan");
    println!("{:-<60}", "");
    println!("Overall risk : {}", plan.overall_risk);
    println!("Requires approval: {}", plan.requires_approval);
    println!("Targets ({}):", plan.targets.len());
    for target in &plan.targets {
        let status = if target.blocked {
            "BLOCKED"
        } else if target.needs_force {
            "needs --force"
        } else {
            "ok"
        };
        println!(
            "  {}\t{}\t[{}]\t{}s\t{}",
            target.host, target.host_address, target.risk_level, target.timeout_secs, status,
        );
        println!("    command: {}", target.command);
        if let Some(ref jh) = target.jump_host {
            println!("    jump host: {}", jh);
        }
    }
    if !plan.warnings.is_empty() {
        println!("Warnings:");
        for w in &plan.warnings {
            println!("  ! {}", w);
        }
    }
}

fn print_comparison(comparison: &ExecComparison) {
    println!("\n--- Result Comparison ---");
    println!("Hosts compared: {}", comparison.hosts_count);
    println!("\nExit code groups:");
    for group in &comparison.exit_code_groups {
        let code_str = group
            .exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "unknown".into());
        println!("  code {}: {:?}", code_str, group.hosts);
    }
    println!("\nStdout:");
    if comparison.stdout_comparison.identical {
        println!("  Identical across all hosts.");
    } else {
        println!("  Differs across hosts.");
        if !comparison.stdout_comparison.common_prefix.is_empty() {
            println!(
                "  Common prefix: {:?}...",
                comparison.stdout_comparison.common_prefix
            );
        }
        for diff in &comparison.stdout_comparison.diffs {
            let marker = if diff.differs_from_first {
                " (differs)"
            } else {
                ""
            };
            println!("  [{}]{}", diff.host, marker);
            for line in diff.output_summary.lines().take(5) {
                println!("    {}", line);
            }
        }
    }
    println!("\nStderr:");
    if comparison.stderr_comparison.identical {
        println!("  Identical across all hosts.");
    } else {
        println!("  Differs across hosts.");
        for diff in &comparison.stderr_comparison.diffs {
            let marker = if diff.differs_from_first {
                " (differs)"
            } else {
                ""
            };
            println!("  [{}]{}", diff.host, marker);
            for line in diff.output_summary.lines().take(5) {
                println!("    {}", line);
            }
        }
    }
    println!("\nSummary: {}", comparison.summary);
}

fn print_webdav_result(result: &agent2ssh::WebDavSyncResult) {
    println!(
        "WebDAV {} complete: global_version={} sync_id={}",
        result.direction, result.global_version, result.sync_id
    );
    println!("Local backup: {}", result.backup_path);
    println!("Local marker: {}", result.marker_path);
    if result.files.is_empty() {
        println!("Files: none");
    } else {
        println!("Files:");
        for file in &result.files {
            println!("  {}\t{} bytes\t{}", file.path, file.bytes, file.sha256);
        }
    }
}

fn print_webdav_status(status: &agent2ssh::WebDavSyncStatus) {
    fn print_marker(label: &str, marker: &Option<agent2ssh::WebDavSyncMarker>) {
        match marker {
            Some(marker) => {
                println!(
                    "{label}: global_version={} direction={} updated_at={} sync_id={}",
                    marker.global_version, marker.direction, marker.updated_at, marker.sync_id
                );
                println!("  files: {}", marker.files.len());
            }
            None => println!("{label}: not initialized"),
        }
    }

    print_marker("Local", &status.local);
    print_marker("Remote", &status.remote);
}

async fn check_daemon_health() -> bool {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    match client
        .get(format!("{}/health", agent2ssh::local_daemon_url()))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

async fn daemon_health_body() -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let response = client
        .get(format!("{}/health", agent2ssh::local_daemon_url()))
        .send()
        .await
        .context("failed to reach daemon health endpoint")?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("failed to read daemon health response")?;
    if status.is_success() {
        Ok(body)
    } else {
        Err(anyhow::anyhow!("daemon health failed ({status}): {body}"))
    }
}

async fn update_gate(
    alias: Option<&str>,
    action: &str,
    reason: Option<String>,
) -> Result<ExecutionGateStatus> {
    let (client, base_url, token) = daemon_client(alias)?;
    let status = daemon_json(
        client
            .post(format!("{base_url}/gate/{action}"))
            .bearer_auth(token)
            .json(&GateUpdateRequest {
                source: source_from_transport(),
                reason,
            }),
    )
    .await?;
    Ok(status)
}

fn print_gate_status(status: &ExecutionGateStatus, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(status)?);
    } else {
        println!("Execution gate: {}", status.mode);
        if let Some(updated_at) = status.updated_at.as_ref() {
            println!("Updated at: {}", updated_at);
        }
        if let Some(updated_by) = status.updated_by.as_deref() {
            println!("Updated by: {}", updated_by);
        }
        if let Some(reason) = status.reason.as_deref() {
            println!("Reason: {}", reason);
        }
    }
    Ok(())
}

fn daemon_client(alias: Option<&str>) -> Result<(reqwest::Client, String, String)> {
    let alias = alias.unwrap_or("localhost");
    let (url, token) = get_daemon(alias)?;
    let token = token.with_context(|| {
        if alias == "localhost" {
            "local daemon token not found; run `agent2ssh daemon start` before using session/forward CLI commands"
                .to_string()
        } else {
            format!("no token configured for daemon '{alias}'")
        }
    })?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    Ok((client, url.trim_end_matches('/').to_string(), token))
}

async fn daemon_json<T: serde::de::DeserializeOwned>(
    request: reqwest::RequestBuilder,
) -> Result<T> {
    let response = request
        .send()
        .await
        .context("failed to reach daemon; run `agent2ssh daemon start` or pass --daemon <alias>")?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("daemon request failed ({status}): {body}");
    }
    response
        .json::<T>()
        .await
        .context("failed to parse daemon response")
}
