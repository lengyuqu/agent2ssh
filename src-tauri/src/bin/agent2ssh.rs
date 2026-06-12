use agent2ssh::remote::get_daemon;
use agent2ssh::remote::{check_daemon_version, diagnose_daemon, get_daemons_unified_view, PROTOCOL_VERSION};
use agent2ssh::store::{audit_path, compute_metrics_trend, restrict_file_to_owner, TrendPeriod};
use agent2ssh::events::subscribe_events;
use agent2ssh::{
    add_host_core, classify_risk, collect_health_snapshot, compare_exec_results, dry_run_playbook,
    exec_multi_core, exec_multi_with_strategy, exec_ssh_core, export_audit_csv,
    export_audit_jsonl, export_team_config, filter_hosts, import_ssh_config_core,
    import_team_config, list_audit_core, list_daemons_core, list_hosts_filtered_core,
    ping_hosts_core, preview_exec, preview_exec_multi, remove_host_core, run_playbook_core,
    sftp_download_core, sftp_ls_core, sftp_mkdir_core, sftp_stat_core, sftp_upload_core,
    AuditFilter, BatchStrategy, ExecComparison, ExecRequest, ForwardDirection, ForwardRule,
    HostFilter, HostProfile, RiskLevel, SftpDownloadRequest, SftpUploadRequest, TeamConfigExport,
};
use agent2ssh::approval::{
    check_approval_required, list_approval_policies, load_approval_policies,
    save_approval_policies, ApprovalPolicy,
};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Parser)]
#[command(name = "agent2ssh", version)]
#[command(about = "SSH capability layer for agents")]
struct Cli {
    /// Route operations through a remote daemon by alias (from ~/.agent2ssh/remotes.toml).
    /// Use "localhost" or omit for the local daemon.
    #[arg(long, global = true)]
    daemon: Option<String>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Host {
        #[command(subcommand)]
        command: HostCommands,
    },
    Exec {
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
        #[arg(required = true, num_args = 1..)]
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
    /// Check SSH reachability of one or more hosts
    Ping {
        #[arg(required = true, num_args = 1..)]
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
        #[arg(long)]
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
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Manage the daemon process
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
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
    /// Run diagnostic checks on the agent2ssh environment
    Doctor {
        /// Output results as JSON
        #[arg(long)]
        json: bool,
        /// Run diagnostics against a specific remote daemon (by alias from remotes.toml)
        #[arg(long)]
        daemon: Option<String>,
    },
    /// Collect health snapshot (uptime, disk, memory, load) for configured hosts
    Health {
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Hosts to collect health from (default: all configured hosts)
        #[arg(long)]
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
        /// Host profile alias to use as ProxyJump bastion
        #[arg(long)]
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
        #[arg(long)]
        json: bool,
    },
    Rm {
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
        host: String,
        local: String,
        remote: String,
        #[arg(long)]
        json: bool,
    },
    /// Download a remote file to local path
    Get {
        host: String,
        remote: String,
        local: String,
        #[arg(long)]
        json: bool,
    },
    /// List a remote directory
    Ls {
        host: String,
        path: String,
        #[arg(long)]
        json: bool,
    },
    /// Stat a remote file or directory
    Stat {
        host: String,
        path: String,
        #[arg(long)]
        json: bool,
    },
    /// Create a remote directory (mkdir -p)
    Mkdir {
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
        host: String,
        #[arg(long)]
        json: bool,
    },
    /// Write input to an open session
    Write { session_id: String, input: String },
    /// Read buffered output from a session
    Read {
        session_id: String,
        #[arg(long, default_value_t = 2000)]
        timeout_ms: u64,
        #[arg(long)]
        json: bool,
    },
    /// Close a session
    Close {
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
        id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Serialize)]
struct SessionOpenRequest {
    host: String,
}

#[derive(Debug, Serialize)]
struct SessionWriteRequest {
    input: String,
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
        #[arg(long, value_delimiter = ',')]
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
        name: String,
        /// Target host profile alias
        #[arg(long)]
        host: String,
        /// Required for high-risk steps
        #[arg(long)]
        force: bool,
        /// Parameters as key=value pairs (repeatable)
        #[arg(long = "params", value_name = "KEY=VALUE")]
        params: Option<Vec<String>>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show resolved commands without executing (dry run)
    DryRun {
        /// Playbook name to preview
        name: String,
        /// Parameters as key=value pairs (repeatable)
        #[arg(long = "params", value_name = "KEY=VALUE")]
        params: Option<Vec<String>>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let daemon_alias = cli.daemon.clone();

    match cli.command {
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
                let hosts = list_hosts_filtered_core(&filter)?;
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
                jump,
                risk_override,
                tags,
                env,
                role,
                owner,
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
                    jump_host: jump,
                    risk_override,
                    tags: tags.unwrap_or_default(),
                    env: clean_optional(env),
                    role: clean_optional(role),
                    owner: clean_optional(owner),
                })?;
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

            let risk = classify_risk(&command);
            let req = ExecRequest {
                host,
                command,
                force,
                timeout_secs,
                stdin,
                max_output_bytes: None,
                reason,
                change_id,
            };

            // If --daemon is set and remote, forward via HTTP
            if let Some(ref alias) = daemon_alias {
                if alias != "localhost" {
                    let (url, token) = get_daemon(alias)?;
                    let token_val =
                        token.ok_or_else(|| anyhow::anyhow!("no token for daemon '{alias}'"))?;
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
                let result = exec_ssh_core(req).await?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
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
            reason: _,
            change_id: _,
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

            if has_strategy {
                let strategy = BatchStrategy {
                    concurrency,
                    max_failures,
                    batch_size,
                    pause_between_batches_secs: pause_secs,
                };
                let batch_result = exec_multi_with_strategy(
                    hosts, command, force, timeout_secs, tags, Some(strategy),
                )
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
                let results = exec_multi_core(hosts, command, force, timeout_secs, tags).await;
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
                json,
            } => {
                let result = sftp_upload_core(SftpUploadRequest {
                    host,
                    local_path: local,
                    remote_path: remote,
                })
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
                json,
            } => {
                let result = sftp_download_core(SftpDownloadRequest {
                    host,
                    remote_path: remote,
                    local_path: local,
                })
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
                let result = sftp_ls_core(&host, &path, None).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    print!("{}", result.stdout);
                }
            }
            SftpCommands::Stat { host, path, json } => {
                let result = sftp_stat_core(&host, &path, None).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    print!("{}", result.stdout);
                }
            }
            SftpCommands::Mkdir { host, path, json } => {
                let result = sftp_mkdir_core(&host, &path, None).await?;
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
                        .json(&SessionWriteRequest { input }),
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
                json,
            } => {
                let dir = match direction.as_str() {
                    "remote" => ForwardDirection::Remote,
                    _ => ForwardDirection::Local,
                };
                let (client, base_url, token) = daemon_client(daemon_alias.as_deref())?;
                let rule: ForwardRule = daemon_json(
                    client
                        .post(format!("{base_url}/forwards"))
                        .bearer_auth(token)
                        .json(&ForwardRule {
                            id: uuid::Uuid::new_v4(),
                            host,
                            direction: dir,
                            bind_port,
                            target_host,
                            target_port,
                        }),
                )
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
                        anyhow::bail!("unsupported export format '{}', expected 'jsonl' or 'csv'", other);
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
            host: _,
            json,
        } => {
            let risk = classify_risk(&command);
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
            let pid_path = config_dir.join("daemon.pid");
            match command {
                DaemonCommands::Start => {
                    if pid_path.exists() {
                        let pid = std::fs::read_to_string(&pid_path)?.trim().to_string();
                        // Check if process is alive
                        if process_is_alive(&pid) {
                            println!("Daemon is already running (pid={})", pid);
                            return Ok(());
                        }
                    }
                    // Start daemon as background process
                    let exe = std::env::current_exe()?;
                    let daemon_bin = exe.parent().unwrap().join("agent2ssh-daemon");
                    if !daemon_bin.exists() {
                        println!("Daemon binary not found: {}", daemon_bin.display());
                        return Ok(());
                    }
                    std::process::Command::new(&daemon_bin)
                        .spawn()
                        .map_err(|e| anyhow::anyhow!("Failed to start daemon: {}", e))?;
                    println!("Daemon started.");
                }
                DaemonCommands::Stop => {
                    if !pid_path.exists() {
                        println!("Daemon is not running (no PID file).");
                        return Ok(());
                    }
                    let pid = std::fs::read_to_string(&pid_path)?.trim().to_string();
                    let _ = std::process::Command::new("kill").arg(&pid).status();
                    let _ = std::fs::remove_file(&pid_path);
                    println!("Daemon stopped (pid={}).", pid);
                }
                DaemonCommands::Status => {
                    if !pid_path.exists() {
                        println!("Daemon is not running.");
                        return Ok(());
                    }
                    let pid = std::fs::read_to_string(&pid_path)?.trim().to_string();
                    // Check health endpoint
                    let status = std::process::Command::new("curl")
                        .arg("-s")
                        .arg("http://127.0.0.1:7722/health")
                        .output();
                    match status {
                        Ok(output) if output.status.success() => {
                            println!("Daemon is running (pid={}).", pid);
                            println!("{}", String::from_utf8_lossy(&output.stdout));
                        }
                        _ => {
                            println!(
                                "Daemon PID file exists (pid={}) but health check failed.",
                                pid
                            );
                        }
                    }
                }
                DaemonCommands::Restart => {
                    // Stop first
                    if pid_path.exists() {
                        let pid = std::fs::read_to_string(&pid_path)?.trim().to_string();
                        let _ = std::process::Command::new("kill").arg(&pid).status();
                        let _ = std::fs::remove_file(&pid_path);
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                    // Then start
                    let exe = std::env::current_exe()?;
                    let daemon_bin = exe.parent().unwrap().join("agent2ssh-daemon");
                    std::process::Command::new(&daemon_bin)
                        .spawn()
                        .map_err(|e| anyhow::anyhow!("Failed to start daemon: {}", e))?;
                    println!("Daemon restarted.");
                }
                DaemonCommands::RotateToken { json } => {
                    if pid_path.exists() {
                        let pid = std::fs::read_to_string(&pid_path)?.trim().to_string();
                        if process_is_alive(&pid) {
                            anyhow::bail!(
                                "daemon is running (pid={pid}); stop it before rotating daemon.token"
                            );
                        }
                        let _ = std::fs::remove_file(&pid_path);
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
                                    } else if compat.remote_version.as_deref() != Some(PROTOCOL_VERSION) {
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
                                    scope_parts.push(format!("hosts={}", scope.allowed_hosts.len()));
                                }
                                if !scope.allowed_tags.is_empty() {
                                    scope_parts.push(format!("tags={}", scope.allowed_tags.len()));
                                }
                                if !scope.allowed_commands.is_empty() {
                                    scope_parts.push(format!("cmds={}", scope.allowed_commands.len()));
                                }
                                if !scope.denied_commands.is_empty() {
                                    scope_parts.push(format!("denied={}", scope.denied_commands.len()));
                                }
                                if !scope_parts.is_empty() {
                                    line.push_str(&format!("\t[scope: {}]", scope_parts.join(", ")));
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
        Commands::ConfigImport { path, json, preview } => {
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
        Commands::SshSync { diff, export, path, json } => {
            let ssh_path = path.as_deref();
            if diff || (!diff && !export) {
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
                            println!("    {} {}: '{}' (agent2ssh) vs '{}' (ssh config)",
                                c.name, c.field, c.agent2ssh_value, c.ssh_config_value);
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
                    println!("{}", serde_json::json!({ "path": out_path, "hosts_exported": count }));
                } else {
                    println!("Exported {} hosts to {}", count, out_path);
                }
            }
        }
        Commands::Doctor { json: output_json, daemon } => {
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
                    let config = agent2ssh::store::load_config()
                        .context("failed to load configuration")?;
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
                        let pattern = p
                            .command_pattern
                            .as_deref()
                            .unwrap_or("*");
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
            PolicyCommands::Check { host, command, json } => {
                // Look up host tags from config
                let host_tags: Vec<String> = list_hosts_filtered_core(&HostFilter::default())
                    .unwrap_or_default()
                    .iter()
                    .find(|h| h.name == host)
                    .map(|h| h.tags.clone())
                    .unwrap_or_default();

                let risk = classify_risk(&command);
                let result = check_approval_required(&host, &host_tags, &command, risk)?;
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
                        println!(
                            "{}\t{} step(s)\t{}",
                            pb.name,
                            step_count,
                            pb.description
                        );
                    }
                }
            }
            PlaybookCommands::Run {
                name,
                host,
                force,
                params,
                json,
            } => {
                let params_map = parse_cli_params(params);
                let result = run_playbook_core(
                    &name,
                    &host,
                    force,
                    if params_map.is_empty() {
                        None
                    } else {
                        Some(&params_map)
                    },
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

fn process_is_alive(pid: &str) -> bool {
    #[cfg(unix)]
    {
        matches!(
            std::process::Command::new("kill").arg("-0").arg(pid).status(),
            Ok(status) if status.success()
        )
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
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

    // 1. Check `ssh` binary exists
    let ssh_ok = which_exists("ssh");
    checks.push(DoctorCheck {
        name: "ssh binary".into(),
        status: if ssh_ok { "pass" } else { "fail" }.into(),
        detail: if ssh_ok {
            "ssh found in PATH".into()
        } else {
            "ssh binary not found in PATH".into()
        },
    });

    // 2. Check `ssh-keygen` exists
    let keygen_ok = which_exists("ssh-keygen");
    checks.push(DoctorCheck {
        name: "ssh-keygen binary".into(),
        status: if keygen_ok { "pass" } else { "warn" }.into(),
        detail: if keygen_ok {
            "ssh-keygen found in PATH".into()
        } else {
            "ssh-keygen not found (key generation unavailable)".into()
        },
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
            "daemon not reachable on 127.0.0.1:7722".into()
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

fn which_exists(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
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
                &comparison.stdout_comparison.common_prefix
            );
        }
        for diff in &comparison.stdout_comparison.diffs {
            let marker = if diff.differs_from_first { " (differs)" } else { "" };
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
            let marker = if diff.differs_from_first { " (differs)" } else { "" };
            println!("  [{}]{}", diff.host, marker);
            for line in diff.output_summary.lines().take(5) {
                println!("    {}", line);
            }
        }
    }
    println!("\nSummary: {}", comparison.summary);
}

async fn check_daemon_health() -> bool {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    match client.get("http://127.0.0.1:7722/health").send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
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
