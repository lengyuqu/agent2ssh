use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::{process::Stdio, time::Duration, time::Instant};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, process::Command, task::JoinSet};

use crate::{
    connection::{apply_socket, get_or_create_socket},
    store::{append_audit, hosts_lock, list_audit_raw, load_config, save_config},
    types::{
        AuditEntry, AuditFilter, ExecMultiResult, ExecRequest, ExecResult, HostProfile,
        PingResult, RiskLevel, SftpDirection, SftpDownloadRequest, SftpResult, SftpUploadRequest,
    },
};

// ── Team Config Export / Import ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TeamConfigExport {
    /// Host profiles with key_path stripped for safe sharing
    pub hosts: Vec<HostProfile>,
    /// Raw TOML content of risk_rules.toml (if it exists)
    pub risk_rules: Option<String>,
    /// Raw TOML content of playbooks.toml (if it exists)
    pub playbooks: Option<String>,
}

/// Export team configuration: hosts (without private key paths), risk rules,
/// and playbooks. Suitable for sharing within a team.
pub fn export_team_config() -> Result<TeamConfigExport> {
    let config = load_config()?;

    // Strip key_path from all hosts
    let hosts: Vec<HostProfile> = config
        .hosts
        .into_iter()
        .map(|mut h| {
            h.key_path = None;
            h
        })
        .collect();

    let config_dir = crate::store::config_dir()?;

    let risk_rules_path = config_dir.join("risk_rules.toml");
    let risk_rules = if risk_rules_path.exists() {
        Some(
            std::fs::read_to_string(&risk_rules_path)
                .with_context(|| format!("failed to read {}", risk_rules_path.display()))?,
        )
    } else {
        None
    };

    let playbooks_path = config_dir.join("playbooks.toml");
    let playbooks = if playbooks_path.exists() {
        Some(
            std::fs::read_to_string(&playbooks_path)
                .with_context(|| format!("failed to read {}", playbooks_path.display()))?,
        )
    } else {
        None
    };

    Ok(TeamConfigExport {
        hosts,
        risk_rules,
        playbooks,
    })
}

/// Import team configuration: merge hosts (skip duplicates by name), and
/// optionally overwrite risk rules and playbooks.
pub fn import_team_config(export: &TeamConfigExport) -> Result<ImportResult> {
    let _guard = hosts_lock().lock().unwrap();
    let mut config = load_config()?;

    let existing_names: std::collections::HashSet<String> =
        config.hosts.iter().map(|h| h.name.clone()).collect();

    let mut added = 0u32;
    let mut skipped = 0u32;
    for host in &export.hosts {
        if existing_names.contains(&host.name) {
            skipped += 1;
        } else {
            config.hosts.push(host.clone());
            added += 1;
        }
    }
    config.hosts.sort_by(|a, b| a.name.cmp(&b.name));
    save_config(&config)?;

    let config_dir = crate::store::config_dir()?;

    if let Some(ref rules) = export.risk_rules {
        let path = config_dir.join("risk_rules.toml");
        std::fs::write(&path, rules)
            .with_context(|| format!("failed to write {}", path.display()))?;
    }

    if let Some(ref pbs) = export.playbooks {
        let path = config_dir.join("playbooks.toml");
        std::fs::write(&path, pbs)
            .with_context(|| format!("failed to write {}", path.display()))?;
    }

    Ok(ImportResult {
        hosts_added: added,
        hosts_skipped: skipped,
        risk_rules_imported: export.risk_rules.is_some(),
        playbooks_imported: export.playbooks.is_some(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub hosts_added: u32,
    pub hosts_skipped: u32,
    pub risk_rules_imported: bool,
    pub playbooks_imported: bool,
}

pub fn list_hosts_core() -> Result<Vec<HostProfile>> {
    Ok(load_config()?.hosts)
}

pub fn add_host_core(host: HostProfile) -> Result<HostProfile> {
    validate_host(&host)?;
    let _guard = hosts_lock().lock().unwrap();
    let mut config = load_config()?;
    if let Some(existing) = config.hosts.iter_mut().find(|item| item.name == host.name) {
        *existing = host.clone();
    } else {
        config.hosts.push(host.clone());
    }
    config.hosts.sort_by(|a, b| a.name.cmp(&b.name));
    save_config(&config)?;
    Ok(host)
}

pub fn remove_host_core(name: &str) -> Result<()> {
    let _guard = hosts_lock().lock().unwrap();
    let mut config = load_config()?;
    let before = config.hosts.len();
    config.hosts.retain(|h| h.name != name);
    if config.hosts.len() == before {
        return Err(anyhow!("no host profile named '{name}'"));
    }
    save_config(&config)
}

pub fn list_audit_core(filter: AuditFilter) -> Result<Vec<AuditEntry>> {
    list_audit_raw(&filter)
}

pub fn classify_risk(command: &str) -> RiskLevel {
    let lower = command.trim().to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    let first = tokens.first().copied().unwrap_or("");

    // ── BLOCKED ──────────────────────────────────────────────────────────────
    // mkfs destroys filesystems
    if lower.contains("mkfs") {
        return RiskLevel::Blocked;
    }
    // fork bomb
    if lower.contains(":(){ :|:& }") || lower.contains(":(){ :|: & }") {
        return RiskLevel::Blocked;
    }
    // dd writing to raw block device
    if first == "dd"
        && (lower.contains("of=/dev/sd")
            || lower.contains("of=/dev/nvme")
            || lower.contains("of=/dev/xvd")
            || lower.contains("of=/dev/disk"))
    {
        return RiskLevel::Blocked;
    }
    // direct redirect to block device
    if lower.contains("> /dev/sd")
        || lower.contains("> /dev/nvme")
        || lower.contains("> /dev/xvd")
    {
        return RiskLevel::Blocked;
    }
    // rm -rf / or rm -rf /* (filesystem wipe)
    if first == "rm" || (first == "sudo" && tokens.get(1).copied() == Some("rm")) {
        let is_recursive = lower.contains("-rf")
            || lower.contains("-fr")
            || lower.contains("-r -f")
            || lower.contains("-f -r");
        if is_recursive {
            let last = tokens.last().copied().unwrap_or("");
            if last == "/" || last == "/*" || last == "/." {
                return RiskLevel::Blocked;
            }
        }
    }
    // halt / shutdown / poweroff / reboot
    let shutdown_cmds = ["shutdown", "halt", "poweroff", "reboot"];
    if shutdown_cmds.contains(&first) {
        return RiskLevel::Blocked;
    }
    if first == "sudo" {
        if let Some(second) = tokens.get(1) {
            if shutdown_cmds.contains(second) {
                return RiskLevel::Blocked;
            }
        }
    }
    // init 0 / init 6
    if first == "init" {
        if matches!(tokens.get(1).copied(), Some("0") | Some("6")) {
            return RiskLevel::Blocked;
        }
    }

    // ── HIGH ─────────────────────────────────────────────────────────────────
    // sudo elevates anything
    if lower.contains("sudo ") {
        return RiskLevel::High;
    }
    // rm -r / rm -rf (not root, but still dangerous)
    if first == "rm"
        && (lower.contains("-rf")
            || lower.contains("-fr")
            || lower.contains(" -r ")
            || lower.ends_with(" -r"))
    {
        return RiskLevel::High;
    }
    // dangerous kill
    if lower.contains("kill -9 -1") || lower.contains("killall -9") || lower.contains("pkill -9") {
        return RiskLevel::High;
    }
    // firewall teardown
    if lower.contains("iptables -f") || lower.contains("ufw disable") || lower.contains("ufw reset") {
        return RiskLevel::High;
    }
    // world-writeable chmod
    if lower.contains("chmod 777") || lower.contains("chmod -r 777") || lower.contains("chmod a+rwx") {
        return RiskLevel::High;
    }
    // account management
    if matches!(first, "passwd" | "useradd" | "userdel" | "usermod" | "chpasswd") {
        return RiskLevel::High;
    }
    // writing to critical system paths
    if lower.contains("> /etc/")
        || lower.contains("> /proc/")
        || lower.contains("> /sys/")
        || lower.contains("> /boot/")
    {
        return RiskLevel::High;
    }
    // service stop
    if lower.contains("systemctl stop") || lower.contains("systemctl kill") {
        return RiskLevel::High;
    }
    if lower.contains("service ") && lower.contains(" stop") {
        return RiskLevel::High;
    }
    // SQL destructive
    if lower.contains("drop table")
        || lower.contains("drop database")
        || lower.contains("drop schema")
        || lower.contains("truncate table")
    {
        return RiskLevel::High;
    }
    // recursive chown
    if (first == "chown") && (lower.contains("-r") || lower.contains("--recursive")) {
        return RiskLevel::High;
    }

    // ── MEDIUM ───────────────────────────────────────────────────────────────
    let medium_contains: &[&str] = &[
        "apt install", "apt-get install", "yum install", "dnf install",
        "pip install", "pip3 install", "npm install", "brew install",
        "systemctl restart", "systemctl enable", "systemctl disable",
        "systemctl start",
        "sed -i",
        "git push",
        "chmod",
        "chown",
        "unzip", "tar -x", "tar xf", "tar xvf", "tar xzf",
        "curl -o ", "wget -o ",
        "truncate",
        "mv /",
    ];
    for pat in medium_contains {
        if lower.contains(pat) {
            return RiskLevel::Medium;
        }
    }
    // Output redirect (overwrite), but not append
    if lower.contains(" > ") {
        return RiskLevel::Medium;
    }
    if matches!(first, "service") {
        return RiskLevel::Medium;
    }

    RiskLevel::Low
}

pub async fn exec_ssh_core(request: ExecRequest) -> Result<ExecResult> {
    exec_ssh_core_with_risk_override(request, None).await
}

pub fn apply_risk_override(classified: RiskLevel, override_level: Option<RiskLevel>) -> RiskLevel {
    if classified == RiskLevel::Blocked {
        RiskLevel::Blocked
    } else {
        override_level.unwrap_or(classified)
    }
}

pub(crate) async fn exec_ssh_core_with_risk_override(
    request: ExecRequest,
    request_risk_override: Option<RiskLevel>,
) -> Result<ExecResult> {
    let host = resolve_host(&request.host)?;

    // Risk overrides are reserved for explicitly trusted scopes such as a host
    // profile or a playbook. They never downgrade a blocked command.
    let built_in_risk = classify_risk(&request.command);
    let classified_risk = {
        // Also check user-defined risk rules from risk_rules.toml.
        if let Some(user_risk) = crate::risk_config::classify_with_user_rules(&request.command).await {
            // User rules escalate but never de-escalate below built-in.
            match (&user_risk, &built_in_risk) {
                (RiskLevel::Blocked, _) => RiskLevel::Blocked,
                (RiskLevel::High, RiskLevel::Blocked) => RiskLevel::Blocked,
                (ur, _) => *ur,
            }
        } else {
            built_in_risk
        }
    };
    let risk = apply_risk_override(classified_risk, request_risk_override.or(host.risk_override));

    if risk == RiskLevel::Blocked {
        return Err(anyhow!(
            "command blocked (risk=blocked): '{}' is unconditionally dangerous",
            request.command
        ));
    }
    if risk == RiskLevel::High && !request.force {
        return Err(anyhow!(
            "command requires force=true (risk=high): '{}'",
            request.command
        ));
    }
    let started = Instant::now();
    let timeout_secs = request.timeout_secs.unwrap_or(60);

    const DEFAULT_MAX_OUTPUT: usize = 4 * 1024 * 1024; // 4 MiB
    let max_bytes = request.max_output_bytes.unwrap_or(DEFAULT_MAX_OUTPUT);

    let mut cmd = build_ssh_command(&host);
    // Reuse an existing ControlMaster connection when available
    if let Some(socket) = get_or_create_socket(&host).await {
        apply_socket(&mut cmd, &socket);
    }
    cmd.arg(ssh_target(&host)).arg(&request.command);

    if request.stdin.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd.spawn().context("failed to spawn ssh")?;

    if let Some(data) = &request.stdin {
        if let Some(mut handle) = child.stdin.take() {
            handle.write_all(data.as_bytes()).await.context("failed to write stdin")?;
        }
    }

    // Read stdout (capped) and stderr concurrently, then wait for exit status.
    let stdout_handle = child.stdout.take().context("no stdout")?;
    let mut stderr_handle = child.stderr.take().context("no stderr")?;

    let (raw_stdout, raw_stderr, status) = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        async {
            // AsyncReadExt::take limits stdout to max_bytes before reading to end
            let mut stdout_capped = stdout_handle.take(max_bytes as u64);
            let (out_res, err_res) = tokio::join!(
                async {
                    let mut buf = Vec::new();
                    stdout_capped.read_to_end(&mut buf).await.map(|_| buf)
                },
                async {
                    let mut buf = Vec::new();
                    stderr_handle.read_to_end(&mut buf).await.map(|_| buf)
                },
            );
            let status = child.wait().await?;
            anyhow::Ok((out_res?, err_res?, status))
        },
    )
    .await
    .map_err(|_| anyhow!("SSH command timed out after {timeout_secs}s: '{}'", request.command))?
    .context("failed waiting for ssh")?;

    // Truncated when stdout hit the cap (take stops at exactly max_bytes)
    let truncated = raw_stdout.len() >= max_bytes;
    let result = ExecResult {
        host: request.host,
        command: request.command,
        exit_code: status.code(),
        stdout: String::from_utf8_lossy(&raw_stdout).into_owned(),
        stderr: String::from_utf8_lossy(&raw_stderr).into_owned(),
        duration_ms: started.elapsed().as_millis(),
        risk_level: risk,
        truncated,
    };
    append_audit(&result, risk)?;
    Ok(result)
}

pub async fn exec_multi_core(
    hosts: Vec<String>,
    command: String,
    force: bool,
    timeout_secs: Option<u64>,
    tags: Option<Vec<String>>,
) -> Vec<ExecMultiResult> {
    // Expand tags into host names
    let resolved_hosts = if let Some(tag_list) = tags {
        if !tag_list.is_empty() {
            let config = match crate::store::load_config() {
                Ok(c) => c,
                Err(_) => return vec![],
            };
            let mut expanded = hosts.clone();
            for h in &config.hosts {
                if h.tags.iter().any(|t| tag_list.contains(t)) {
                    if !expanded.contains(&h.name) {
                        expanded.push(h.name.clone());
                    }
                }
            }
            expanded
        } else {
            hosts
        }
    } else {
        hosts
    };

    let mut set = JoinSet::new();

    for host in resolved_hosts {
        let cmd = command.clone();
        set.spawn(async move {
            let req = ExecRequest {
                host: host.clone(),
                command: cmd,
                force,
                timeout_secs,
                stdin: None,
                max_output_bytes: None,
            };
            match exec_ssh_core(req).await {
                Ok(r) => ExecMultiResult { host, result: Some(r), error: None },
                Err(e) => ExecMultiResult { host, result: None, error: Some(e.to_string()) },
            }
        });
    }

    let mut results = Vec::new();
    while let Some(joined) = set.join_next().await {
        results.push(joined.unwrap_or_else(|e| ExecMultiResult {
            host: "unknown".into(),
            result: None,
            error: Some(format!("task panicked: {e}")),
        }));
    }
    results
}

fn build_ssh_command(host: &HostProfile) -> Command {
    let mut cmd = Command::new("ssh");
    cmd.arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new")
        .arg("-p")
        .arg(host.port.unwrap_or(22).to_string());
    if let Some(key_path) = &host.key_path {
        if !key_path.trim().is_empty() {
            cmd.arg("-i").arg(expand_tilde(key_path));
        }
    }
    // ProxyJump: resolve the jump host profile and build a -J argument
    if let Some(jump_name) = &host.jump_host {
        if let Ok(jump) = resolve_host(jump_name) {
            let jump_target = match &jump.user {
                Some(u) if !u.trim().is_empty() => {
                    format!("{}@{}:{}", u, jump.host, jump.port.unwrap_or(22))
                }
                _ => format!("{}:{}", jump.host, jump.port.unwrap_or(22)),
            };
            cmd.arg("-J").arg(jump_target);
            // Also pass the jump host's key if specified
            if let Some(jkey) = &jump.key_path {
                if !jkey.trim().is_empty() {
                    cmd.arg("-i").arg(expand_tilde(jkey));
                }
            }
        }
    }
    cmd
}

fn validate_host(host: &HostProfile) -> Result<()> {
    if host.name.trim().is_empty() {
        return Err(anyhow!("host alias is required"));
    }
    if host.host.trim().is_empty() {
        return Err(anyhow!("host address is required"));
    }
    Ok(())
}

fn resolve_host(name: &str) -> Result<HostProfile> {
    load_config()?
        .hosts
        .into_iter()
        .find(|host| host.name == name)
        .ok_or_else(|| anyhow!("unknown host profile: {name}"))
}

fn ssh_target(host: &HostProfile) -> String {
    match &host.user {
        Some(user) if !user.trim().is_empty() => format!("{user}@{}", host.host),
        _ => host.host.clone(),
    }
}

fn expand_tilde(path: &str) -> String {
    if path == "~" {
        return dirs::home_dir()
            .map(|home| home.display().to_string())
            .unwrap_or_else(|| path.to_string());
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).display().to_string();
        }
    }
    path.to_string()
}

fn scp_base_args(host: &HostProfile) -> Vec<String> {
    let mut args = vec![
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-P".to_string(),
        host.port.unwrap_or(22).to_string(),
    ];
    if let Some(key_path) = &host.key_path {
        if !key_path.trim().is_empty() {
            args.push("-i".to_string());
            args.push(expand_tilde(key_path));
        }
    }
    args
}

pub async fn sftp_upload_core(request: SftpUploadRequest) -> Result<SftpResult> {
    let host = resolve_host(&request.host)?;
    let started = Instant::now();

    let local = expand_tilde(&request.local_path);
    let remote = format!("{}:{}", ssh_target(&host), request.remote_path);

    let mut cmd = Command::new("scp");
    for arg in scp_base_args(&host) {
        cmd.arg(arg);
    }
    cmd.arg(&local)
        .arg(&remote)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd.output().await.context("failed to spawn scp")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("scp upload failed: {stderr}"));
    }

    Ok(SftpResult {
        host: request.host,
        local_path: local,
        remote_path: request.remote_path,
        direction: SftpDirection::Upload,
        duration_ms: started.elapsed().as_millis(),
    })
}

pub async fn sftp_download_core(request: SftpDownloadRequest) -> Result<SftpResult> {
    let host = resolve_host(&request.host)?;
    let started = Instant::now();

    let remote = format!("{}:{}", ssh_target(&host), request.remote_path);
    let local = expand_tilde(&request.local_path);

    let mut cmd = Command::new("scp");
    for arg in scp_base_args(&host) {
        cmd.arg(arg);
    }
    cmd.arg(&remote)
        .arg(&local)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd.output().await.context("failed to spawn scp")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("scp download failed: {stderr}"));
    }

    Ok(SftpResult {
        host: request.host,
        local_path: local,
        remote_path: request.remote_path,
        direction: SftpDirection::Download,
        duration_ms: started.elapsed().as_millis(),
    })
}

// ── SFTP directory operations (via SSH exec) ──────────────────────────────────

pub async fn sftp_ls_core(host_name: &str, path: &str, timeout_secs: Option<u64>) -> Result<ExecResult> {
    exec_ssh_core(ExecRequest {
        host: host_name.to_string(),
        command: format!("ls -la {}", shell_escape(path)),
        force: false,
        timeout_secs,
        stdin: None,
        max_output_bytes: None,
    })
    .await
}

pub async fn sftp_stat_core(host_name: &str, path: &str, timeout_secs: Option<u64>) -> Result<ExecResult> {
    exec_ssh_core(ExecRequest {
        host: host_name.to_string(),
        command: format!("stat {}", shell_escape(path)),
        force: false,
        timeout_secs,
        stdin: None,
        max_output_bytes: None,
    })
    .await
}

pub async fn sftp_mkdir_core(host_name: &str, path: &str, timeout_secs: Option<u64>) -> Result<ExecResult> {
    exec_ssh_core(ExecRequest {
        host: host_name.to_string(),
        command: format!("mkdir -p {}", shell_escape(path)),
        force: false,
        timeout_secs,
        stdin: None,
        max_output_bytes: None,
    })
    .await
}

/// Wraps a path in single quotes and escapes any embedded single quotes.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

// ── SSH config import ─────────────────────────────────────────────────────────

pub fn import_ssh_config_core(path: Option<&str>) -> Result<Vec<HostProfile>> {
    let config_path = match path {
        Some(p) => expand_tilde(p).into(),
        None => dirs::home_dir()
            .ok_or_else(|| anyhow!("cannot locate home directory"))?
            .join(".ssh")
            .join("config"),
    };

    if !config_path.exists() {
        return Ok(Vec::new());
    }

    let raw = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;

    let mut profiles: Vec<HostProfile> = Vec::new();
    let mut current_alias: Option<String> = None;
    let mut hostname: Option<String> = None;
    let mut user: Option<String> = None;
    let mut port: Option<u16> = None;
    let mut identity: Option<String> = None;
    let mut proxy_jump: Option<String> = None;

    let flush = |alias: &Option<String>,
                 hn: &Option<String>,
                 u: &Option<String>,
                 p: Option<u16>,
                 id: &Option<String>,
                 pj: &Option<String>,
                 profiles: &mut Vec<HostProfile>| {
        let alias = alias.as_deref().unwrap_or("");
        let hn = hn.as_deref().unwrap_or("");
        // Skip wildcards and entries without a concrete hostname
        if alias.is_empty() || alias.contains('*') || alias.contains('?') || hn.is_empty() {
            return;
        }
        // Map ProxyJump user@host:port to a profile alias (best-effort: use the host part)
        let jump_host = pj.as_ref().map(|raw| {
            let target = raw.split_whitespace().next().unwrap_or(raw);
            // Strip user@ prefix and :port suffix to get a usable alias hint
            let no_user = target.split('@').last().unwrap_or(target);
            no_user.split(':').next().unwrap_or(no_user).to_string()
        });
        profiles.push(HostProfile {
            name: alias.to_string(),
            host: hn.to_string(),
            user: u.clone(),
            port: p,
            key_path: id.clone(),
            jump_host,
            risk_override: None,
            tags: vec![],
        });
    };

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once(|c: char| c.is_whitespace()) {
            Some(pair) => (pair.0.to_lowercase(), pair.1.trim().to_string()),
            None => continue,
        };
        match key.as_str() {
            "host" => {
                // Flush previous entry
                flush(&current_alias, &hostname, &user, port, &identity, &proxy_jump, &mut profiles);
                current_alias = Some(value);
                hostname = None;
                user = None;
                port = None;
                identity = None;
                proxy_jump = None;
            }
            "hostname" => hostname = Some(value),
            "user" => user = Some(value),
            "port" => port = value.parse().ok(),
            "identityfile" => identity = Some(value),
            "proxyjump" => proxy_jump = Some(value),
            _ => {}
        }
    }
    // Flush last entry
    flush(&current_alias, &hostname, &user, port, &identity, &proxy_jump, &mut profiles);

    // Add only profiles whose name doesn't already exist
    let _guard = hosts_lock().lock().unwrap();
    let mut config = load_config()?;
    let existing: std::collections::HashSet<String> =
        config.hosts.iter().map(|h| h.name.clone()).collect();

    let mut added = Vec::new();
    for p in profiles {
        if !existing.contains(&p.name) {
            added.push(p.clone());
            config.hosts.push(p);
        }
    }
    config.hosts.sort_by(|a, b| a.name.cmp(&b.name));
    save_config(&config)?;
    Ok(added)
}

// ── Ping ─────────────────────────────────────────────────────────────────────

pub async fn ping_hosts_core(host_names: Vec<String>, timeout_secs: Option<u64>) -> Vec<PingResult> {
    let timeout = timeout_secs.unwrap_or(5);
    let mut set = JoinSet::new();

    for name in host_names {
        set.spawn(async move {
            match load_config()
                .ok()
                .and_then(|c| c.hosts.into_iter().find(|h| h.name == name))
            {
                None => PingResult {
                    host: name,
                    reachable: false,
                    latency_ms: None,
                    error: Some("unknown host profile".into()),
                },
                Some(host) => {
                    let started = Instant::now();
                    let target = crate::connection::ssh_target(&host);
                    let mut cmd = build_ssh_command(&host);
                    cmd.arg("-o")
                        .arg(format!("ConnectTimeout={timeout}"))
                        .arg(&target)
                        .arg("true")
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());

                    match tokio::time::timeout(
                        Duration::from_secs(timeout + 2),
                        cmd.status(),
                    )
                    .await
                    {
                        Ok(Ok(status)) if status.success() => PingResult {
                            host: name,
                            reachable: true,
                            latency_ms: Some(started.elapsed().as_millis() as u64),
                            error: None,
                        },
                        Ok(Ok(_)) => PingResult {
                            host: name,
                            reachable: false,
                            latency_ms: Some(started.elapsed().as_millis() as u64),
                            error: Some("SSH handshake failed".into()),
                        },
                        Ok(Err(e)) => PingResult {
                            host: name,
                            reachable: false,
                            latency_ms: None,
                            error: Some(e.to_string()),
                        },
                        Err(_) => PingResult {
                            host: name,
                            reachable: false,
                            latency_ms: None,
                            error: Some(format!("timed out after {timeout}s")),
                        },
                    }
                }
            }
        });
    }

    let mut results = Vec::new();
    while let Some(joined) = set.join_next().await {
        results.push(joined.unwrap_or_else(|e| PingResult {
            host: "unknown".into(),
            reachable: false,
            latency_ms: None,
            error: Some(format!("task panicked: {e}")),
        }));
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_risk_blocked() {
        assert_eq!(classify_risk("mkfs /dev/sda"), RiskLevel::Blocked);
        assert_eq!(classify_risk("rm -rf /"), RiskLevel::Blocked);
        assert_eq!(classify_risk("shutdown"), RiskLevel::Blocked);
        assert_eq!(classify_risk("halt"), RiskLevel::Blocked);
        assert_eq!(classify_risk("poweroff"), RiskLevel::Blocked);
        assert_eq!(classify_risk("reboot"), RiskLevel::Blocked);
        assert_eq!(classify_risk("sudo shutdown"), RiskLevel::Blocked);
        assert_eq!(classify_risk("init 0"), RiskLevel::Blocked);
        assert_eq!(classify_risk("init 6"), RiskLevel::Blocked);
    }

    #[test]
    fn test_classify_risk_high() {
        assert_eq!(classify_risk("sudo whoami"), RiskLevel::High);
        assert_eq!(classify_risk("rm -rf /tmp/stuff"), RiskLevel::High);
        assert_eq!(classify_risk("kill -9 -1"), RiskLevel::High);
        assert_eq!(classify_risk("chmod 777 /etc/passwd"), RiskLevel::High);
        assert_eq!(classify_risk("passwd root"), RiskLevel::High);
        assert_eq!(classify_risk("userdel admin"), RiskLevel::High);
        assert_eq!(classify_risk("iptables -f"), RiskLevel::High);
        assert_eq!(classify_risk("systemctl stop nginx"), RiskLevel::High);
        assert_eq!(classify_risk("drop table users"), RiskLevel::High);
    }

    #[test]
    fn test_classify_risk_medium() {
        assert_eq!(classify_risk("apt install nginx"), RiskLevel::Medium);
        assert_eq!(classify_risk("pip install requests"), RiskLevel::Medium);
        assert_eq!(classify_risk("npm install express"), RiskLevel::Medium);
        assert_eq!(classify_risk("systemctl restart nginx"), RiskLevel::Medium);
        assert_eq!(classify_risk("git push origin main"), RiskLevel::Medium);
        assert_eq!(classify_risk("echo hello > /tmp/file"), RiskLevel::Medium);
        assert_eq!(classify_risk("sed -i 's/foo/bar/' file.txt"), RiskLevel::Medium);
    }

    #[test]
    fn test_classify_risk_low() {
        assert_eq!(classify_risk("ls -la"), RiskLevel::Low);
        assert_eq!(classify_risk("cat /etc/hosts"), RiskLevel::Low);
        assert_eq!(classify_risk("uname -a"), RiskLevel::Low);
        assert_eq!(classify_risk("whoami"), RiskLevel::Low);
        assert_eq!(classify_risk("df -h"), RiskLevel::Low);
        assert_eq!(classify_risk("ps aux"), RiskLevel::Low);
    }

    #[test]
    fn test_classify_risk_dd_blocked() {
        assert_eq!(classify_risk("dd if=/dev/zero of=/dev/sda bs=1M"), RiskLevel::Blocked);
        assert_eq!(classify_risk("dd if=/dev/zero of=/dev/nvme0n1 bs=1M"), RiskLevel::Blocked);
    }

    #[test]
    fn test_risk_override_cannot_downgrade_blocked() {
        assert_eq!(
            apply_risk_override(RiskLevel::Blocked, Some(RiskLevel::Low)),
            RiskLevel::Blocked
        );
        assert_eq!(
            apply_risk_override(RiskLevel::High, Some(RiskLevel::Low)),
            RiskLevel::Low
        );
    }

    #[test]
    fn test_classify_risk_fork_bomb() {
        assert_eq!(classify_risk(":(){ :|:& }"), RiskLevel::Blocked);
        assert_eq!(classify_risk(":(){ :|: & }"), RiskLevel::Blocked);
    }

    #[test]
    fn test_classify_risk_sudo_variants() {
        assert_eq!(classify_risk("sudo rm -rf /"), RiskLevel::Blocked);
        assert_eq!(classify_risk("sudo reboot"), RiskLevel::Blocked);
        assert_eq!(classify_risk("sudo whoami"), RiskLevel::High);
    }

    #[test]
    fn test_classify_risk_case_insensitive() {
        assert_eq!(classify_risk("SUDO whoami"), RiskLevel::High);
        assert_eq!(classify_risk("MKFS /dev/sda"), RiskLevel::Blocked);
        assert_eq!(classify_risk("SHUTDOWN"), RiskLevel::Blocked);
    }

    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("/tmp/file"), "'/tmp/file'");
        assert_eq!(shell_escape("it's a test"), "'it'\\''s a test'");
    }
}
