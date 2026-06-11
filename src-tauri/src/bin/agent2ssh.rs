use agent2ssh::{
    add_host_core, classify_risk, exec_multi_core, exec_ssh_core, forward_add_core,
    forward_list_core, forward_remove_core, import_ssh_config_core, list_audit_core,
    list_hosts_core, ping_hosts_core, remove_host_core, session_close_core, session_list_core,
    session_open_core, session_read_core, session_write_core, sftp_download_core, sftp_ls_core,
    sftp_mkdir_core, sftp_stat_core, sftp_upload_core, AuditFilter, ExecRequest, ForwardDirection,
    HostProfile, RiskLevel, SftpDownloadRequest, SftpUploadRequest,
};
use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "agent2ssh")]
#[command(about = "SSH capability layer for agents")]
struct Cli {
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
}

#[derive(Debug, Subcommand)]
enum HostCommands {
    List {
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
    Write {
        session_id: String,
        input: String,
    },
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Host { command } => match command {
            HostCommands::List { json } => {
                let hosts = list_hosts_core()?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&hosts)?);
                } else if hosts.is_empty() {
                    println!("No hosts configured.");
                } else {
                    for host in hosts {
                        println!(
                            "{}\t{}{}:{}",
                            host.name,
                            host.user.map(|u| format!("{u}@")).unwrap_or_default(),
                            host.host,
                            host.port.unwrap_or(22)
                        );
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
        } => {
            let risk = classify_risk(&command);
            let req = ExecRequest { host, command, force, timeout_secs, stdin, max_output_bytes: None };
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
        Commands::ExecMulti { hosts, command, json, force, timeout_secs, tags } => {
            let results = exec_multi_core(hosts, command, force, timeout_secs, tags).await;
            if json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else {
                for r in &results {
                    match &r.result {
                        Some(res) => println!("[{}] exit={:?} {}ms\n{}", r.host, res.exit_code, res.duration_ms, res.stdout.trim_end()),
                        None => eprintln!("[{}] ERROR: {}", r.host, r.error.as_deref().unwrap_or("unknown")),
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
                let id = session_open_core(&host).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "session_id": id.to_string(), "host": host }))?);
                } else {
                    println!("Session opened: {id}");
                }
            }
            SessionCommands::Write { session_id, input } => {
                let id: uuid::Uuid = session_id.parse()?;
                session_write_core(id, &input).await?;
            }
            SessionCommands::Read { session_id, timeout_ms, json } => {
                let id: uuid::Uuid = session_id.parse()?;
                let output = session_read_core(id, timeout_ms).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "output": output }))?);
                } else {
                    print!("{output}");
                }
            }
            SessionCommands::Close { session_id, json } => {
                let id: uuid::Uuid = session_id.parse()?;
                session_close_core(id).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "closed": session_id }))?);
                } else {
                    println!("Session {session_id} closed.");
                }
            }
            SessionCommands::List { json } => {
                let sessions = session_list_core().await;
                if json {
                    let items: Vec<_> = sessions.iter().map(|(id, host)| serde_json::json!({ "session_id": id.to_string(), "host": host })).collect();
                    println!("{}", serde_json::to_string_pretty(&items)?);
                } else if sessions.is_empty() {
                    println!("No open sessions.");
                } else {
                    for (id, host) in &sessions {
                        println!("{id}\t{host}");
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
                let rule = forward_add_core(&host, dir, bind_port, &target_host, target_port).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&rule)?);
                } else {
                    println!(
                        "Forward {} started: {}:{} → {}:{} (id={})",
                        rule.direction, rule.bind_port, rule.target_host, rule.host, rule.target_port, rule.id
                    );
                }
            }
            ForwardCommands::List { json } => {
                let rules = forward_list_core().await;
                if json {
                    println!("{}", serde_json::to_string_pretty(&rules)?);
                } else if rules.is_empty() {
                    println!("No active forwards.");
                } else {
                    for r in &rules {
                        println!("{}\t{}\t{}\t{}:{} → {}:{}", r.id, r.host, r.direction, r.bind_port, r.target_host, r.host, r.target_port);
                    }
                }
            }
            ForwardCommands::Rm { id, json } => {
                let uid: uuid::Uuid = id.parse()?;
                forward_remove_core(uid).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "removed": id }))?);
                } else {
                    println!("Forward {id} removed.");
                }
            }
        },
        Commands::Ping { hosts, timeout_secs, json } => {
            let results = ping_hosts_core(hosts, Some(timeout_secs)).await;
            if json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else {
                for r in &results {
                    if r.reachable {
                        println!("✓ {} ({}ms)", r.host, r.latency_ms.unwrap_or(0));
                    } else {
                        println!("✗ {}  {}", r.host, r.error.as_deref().unwrap_or("unreachable"));
                    }
                }
            }
        }
        Commands::Audit { limit, json, host, risk, exit_code, since, until } => {
            let filter = AuditFilter {
                host,
                risk_level: risk,
                exit_code,
                since,
                until,
                limit,
            };
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
        Commands::Risk { command, host: _, json } => {
            let risk = classify_risk(&command);
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "command": command,
                    "risk_level": risk,
                }))?);
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
                        if let Ok(_) = std::process::Command::new("kill").arg("-0").arg(&pid).status() {
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
                            println!("Daemon PID file exists (pid={}) but health check failed.", pid);
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
            }
        }
    }

    Ok(())
}
