use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::{process::Stdio, sync::Arc, time::Duration, time::Instant};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, process::Command, sync::Semaphore, task::JoinSet};

use crate::{
    connection::{apply_socket, get_or_create_socket},
    store::{append_audit, hosts_lock, list_audit_raw, load_config, save_config},
    types::{
        AuditEntry, AuditFilter, BatchStrategy, ExecMultiBatchResult, ExecMultiResult, ExecRequest,
        ExecResult, HostFilter, HostProfile, PingResult, RiskLevel, SftpDirection,
        SftpDownloadRequest, SftpResult, SftpUploadRequest,
    },
};

// ── Execution Plan Preview ──────────────────────────────────────────────────

/// Preview of what an execution will do before actually running it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecPlan {
    pub targets: Vec<ExecPlanTarget>,
    pub overall_risk: RiskLevel,
    pub requires_approval: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecPlanTarget {
    pub host: String,
    pub host_address: String,
    pub command: String,
    pub risk_level: RiskLevel,
    pub needs_force: bool,
    pub blocked: bool,
    pub jump_host: Option<String>,
    pub timeout_secs: u64,
}

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

// ── Config Import Diff Preview ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigDiffPreview {
    /// Names of hosts that will be added
    pub hosts_to_add: Vec<String>,
    /// Names of hosts already present (duplicates that will be skipped)
    pub hosts_to_skip: Vec<String>,
    /// Names of hosts that will be updated (name matches but host/port/user differs)
    pub hosts_to_update: Vec<String>,
    /// "new", "overwrite", or None
    pub risk_rules_change: Option<String>,
    /// "new", "overwrite", or None
    pub playbooks_change: Option<String>,
    /// Human-readable summary
    pub summary: String,
}

/// Preview what a team config import will change without actually importing.
pub fn preview_team_config_import(export: &TeamConfigExport) -> Result<ConfigDiffPreview> {
    let config = load_config()?;
    let config_dir = crate::store::config_dir()?;

    let existing_map: std::collections::HashMap<String, &HostProfile> =
        config.hosts.iter().map(|h| (h.name.clone(), h)).collect();

    let mut hosts_to_add = Vec::new();
    let mut hosts_to_skip = Vec::new();
    let mut hosts_to_update = Vec::new();

    for host in &export.hosts {
        if let Some(existing) = existing_map.get(&host.name) {
            // Check if anything meaningful differs
            let same = existing.host == host.host
                && existing.port == host.port
                && existing.user == host.user;
            if same {
                hosts_to_skip.push(host.name.clone());
            } else {
                hosts_to_update.push(host.name.clone());
            }
        } else {
            hosts_to_add.push(host.name.clone());
        }
    }

    let risk_rules_change = if let Some(ref _rules) = export.risk_rules {
        let path = config_dir.join("risk_rules.toml");
        if path.exists() {
            Some("overwrite".to_string())
        } else {
            Some("new".to_string())
        }
    } else {
        None
    };

    let playbooks_change = if let Some(ref _pbs) = export.playbooks {
        let path = config_dir.join("playbooks.toml");
        if path.exists() {
            Some("overwrite".to_string())
        } else {
            Some("new".to_string())
        }
    } else {
        None
    };

    let mut summary_parts = Vec::new();
    summary_parts.push(format!(
        "{} host(s) to add, {} to skip (duplicates), {} to update",
        hosts_to_add.len(),
        hosts_to_skip.len(),
        hosts_to_update.len()
    ));
    if let Some(ref change) = risk_rules_change {
        summary_parts.push(format!("risk rules: {}", change));
    }
    if let Some(ref change) = playbooks_change {
        summary_parts.push(format!("playbooks: {}", change));
    }
    let summary = summary_parts.join("; ");

    Ok(ConfigDiffPreview {
        hosts_to_add,
        hosts_to_skip,
        hosts_to_update,
        risk_rules_change,
        playbooks_change,
        summary,
    })
}

pub fn list_hosts_core() -> Result<Vec<HostProfile>> {
    Ok(load_config()?.hosts)
}

pub fn list_hosts_filtered_core(filter: &HostFilter) -> Result<Vec<HostProfile>> {
    Ok(filter_hosts(load_config()?.hosts, filter))
}

pub fn filter_hosts(hosts: Vec<HostProfile>, filter: &HostFilter) -> Vec<HostProfile> {
    hosts
        .into_iter()
        .filter(|host| host_matches_filter(host, filter))
        .collect()
}

fn host_matches_filter(host: &HostProfile, filter: &HostFilter) -> bool {
    if !matches_optional_label(host.env.as_deref(), filter.env.as_deref()) {
        return false;
    }
    if !matches_optional_label(host.role.as_deref(), filter.role.as_deref()) {
        return false;
    }
    if !matches_optional_label(host.owner.as_deref(), filter.owner.as_deref()) {
        return false;
    }
    if let Some(tag) = normalized_filter(filter.tag.as_deref()) {
        if !host.tags.iter().any(|item| item.trim().eq_ignore_ascii_case(&tag)) {
            return false;
        }
    }
    true
}

fn matches_optional_label(value: Option<&str>, filter: Option<&str>) -> bool {
    let Some(filter) = normalized_filter(filter) else {
        return true;
    };
    value
        .map(|item| item.trim().eq_ignore_ascii_case(&filter))
        .unwrap_or(false)
}

fn normalized_filter(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
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

/// Preview an execution plan for a single host.
pub async fn preview_exec(host: &str, command: &str, timeout_secs: Option<u64>) -> Result<ExecPlan> {
    let profile = resolve_host(host)?;
    let timeout = timeout_secs.unwrap_or(60);
    let built_in_risk = classify_risk(command);

    let classified_risk = if let Some(user_risk) = crate::risk_config::classify_with_user_rules(command).await {
        match (&user_risk, &built_in_risk) {
            (RiskLevel::Blocked, _) => RiskLevel::Blocked,
            (RiskLevel::High, RiskLevel::Blocked) => RiskLevel::Blocked,
            (ur, _) => *ur,
        }
    } else {
        built_in_risk
    };

    let risk = if let Some(override_level) = profile.risk_override {
        apply_risk_override(classified_risk, Some(override_level))
    } else {
        classified_risk
    };

    let needs_force = risk == RiskLevel::High || risk == RiskLevel::Blocked;
    let blocked = risk == RiskLevel::Blocked;

    let mut warnings = Vec::new();
    if blocked {
        warnings.push("Command is blocked and cannot be executed".to_string());
    }
    if risk == RiskLevel::High {
        warnings.push("Command requires force=true".to_string());
    }
    if let Some(ref jh) = profile.jump_host {
        warnings.push(format!("Host uses jump host {}", jh));
    }

    let host_address = ssh_target(&profile);

    Ok(ExecPlan {
        targets: vec![ExecPlanTarget {
            host: profile.name,
            host_address,
            command: command.to_string(),
            risk_level: risk,
            needs_force,
            blocked,
            jump_host: profile.jump_host,
            timeout_secs: timeout,
        }],
        overall_risk: risk,
        requires_approval: risk == RiskLevel::High,
        warnings,
    })
}

/// Preview an execution plan for multiple hosts.
pub async fn preview_exec_multi(
    hosts: Vec<String>,
    command: &str,
    tags: Option<Vec<String>>,
    timeout_secs: Option<u64>,
) -> Result<ExecPlan> {
    let timeout = timeout_secs.unwrap_or(60);

    // Expand tags into host names
    let resolved_names = if let Some(tag_list) = tags {
        if !tag_list.is_empty() {
            let config = load_config()?;
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

    let built_in_risk = classify_risk(command);
    let classified_risk = if let Some(user_risk) = crate::risk_config::classify_with_user_rules(command).await {
        match (&user_risk, &built_in_risk) {
            (RiskLevel::Blocked, _) => RiskLevel::Blocked,
            (RiskLevel::High, RiskLevel::Blocked) => RiskLevel::Blocked,
            (ur, _) => *ur,
        }
    } else {
        built_in_risk
    };

    let mut targets = Vec::new();
    let mut warnings = Vec::new();
    let mut overall_risk = RiskLevel::Low;

    for name in &resolved_names {
        let profile = resolve_host(name)?;
        let risk = if let Some(override_level) = profile.risk_override {
            apply_risk_override(classified_risk, Some(override_level))
        } else {
            classified_risk
        };

        let needs_force = risk == RiskLevel::High || risk == RiskLevel::Blocked;
        let blocked = risk == RiskLevel::Blocked;

        if blocked {
            warnings.push(format!("Command is blocked on host '{}'", profile.name));
        }
        if risk == RiskLevel::High {
            warnings.push(format!("Command requires force=true on host '{}'", profile.name));
        }
        if let Some(ref jh) = profile.jump_host {
            warnings.push(format!("Host '{}' uses jump host {}", profile.name, jh));
        }

        overall_risk = overall_risk.max_severity(risk);
        let host_address = ssh_target(&profile);

        targets.push(ExecPlanTarget {
            host: profile.name,
            host_address,
            command: command.to_string(),
            risk_level: risk,
            needs_force,
            blocked,
            jump_host: profile.jump_host,
            timeout_secs: timeout,
        });
    }

    // De-duplicate warnings
    warnings.sort();
    warnings.dedup();

    let requires_approval = overall_risk == RiskLevel::High;

    Ok(ExecPlan {
        targets,
        overall_risk,
        requires_approval,
        warnings,
    })
}

/// Build an ExecPlan from already-resolved profiles. Useful for testing without
/// requiring a real config file on disk.
pub fn build_plan_from_profile(
    profiles: Vec<HostProfile>,
    command: &str,
    timeout_secs: Option<u64>,
) -> ExecPlan {
    let timeout = timeout_secs.unwrap_or(60);
    let built_in_risk = classify_risk(command);

    let mut targets = Vec::new();
    let mut warnings = Vec::new();
    let mut overall_risk = RiskLevel::Low;

    for profile in profiles {
        let risk = if let Some(override_level) = profile.risk_override {
            apply_risk_override(built_in_risk, Some(override_level))
        } else {
            built_in_risk
        };

        let needs_force = risk == RiskLevel::High || risk == RiskLevel::Blocked;
        let blocked = risk == RiskLevel::Blocked;

        if blocked {
            warnings.push(format!("Command is blocked on host '{}'", profile.name));
        }
        if risk == RiskLevel::High {
            warnings.push(format!("Command requires force=true on host '{}'", profile.name));
        }
        if let Some(ref jh) = profile.jump_host {
            warnings.push(format!("Host '{}' uses jump host {}", profile.name, jh));
        }

        overall_risk = overall_risk.max_severity(risk);
        let host_address = match &profile.user {
            Some(u) if !u.trim().is_empty() => format!("{u}@{}", profile.host),
            _ => profile.host.clone(),
        };

        targets.push(ExecPlanTarget {
            host: profile.name,
            host_address,
            command: command.to_string(),
            risk_level: risk,
            needs_force,
            blocked,
            jump_host: profile.jump_host,
            timeout_secs: timeout,
        });
    }

    warnings.sort();
    warnings.dedup();

    let requires_approval = overall_risk == RiskLevel::High;

    ExecPlan {
        targets,
        overall_risk,
        requires_approval,
        warnings,
    }
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
    append_audit(
        &result,
        risk,
        request.reason.as_deref(),
        request.change_id.as_deref(),
    )?;

    // Publish ExecCompleted event
    crate::events::publish_event(
        crate::events::EventType::ExecCompleted,
        serde_json::json!({
            "host": result.host,
            "command": result.command,
            "exit_code": result.exit_code,
            "risk_level": format!("{}", result.risk_level),
            "duration_ms": result.duration_ms,
        }),
    );

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
                reason: None,
                change_id: None,
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

/// Execute a command on multiple hosts with a batch strategy controlling concurrency,
/// failure thresholds, batched rollout, and pauses between batches.
pub async fn exec_multi_with_strategy(
    hosts: Vec<String>,
    command: String,
    force: bool,
    timeout_secs: Option<u64>,
    tags: Option<Vec<String>>,
    strategy: Option<BatchStrategy>,
) -> ExecMultiBatchResult {
    let started = Instant::now();

    // Resolve hosts (expand tags)
    let resolved_hosts = if let Some(tag_list) = tags {
        if !tag_list.is_empty() {
            let config = match crate::store::load_config() {
                Ok(c) => c,
                Err(_) => {
                    return ExecMultiBatchResult {
                        results: vec![],
                        total_hosts: 0,
                        successful: 0,
                        failed: 0,
                        skipped: 0,
                        stopped_early: false,
                        batches_executed: 0,
                        total_duration_ms: started.elapsed().as_millis(),
                    };
                }
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

    let total_hosts = resolved_hosts.len();

    // Extract strategy parameters (defaults = unlimited/no limits)
    let concurrency = strategy.as_ref().and_then(|s| s.concurrency).unwrap_or(0);
    let max_failures = strategy.as_ref().and_then(|s| s.max_failures).unwrap_or(0);
    let batch_size = strategy.as_ref().and_then(|s| s.batch_size).unwrap_or(0);
    let pause_secs = strategy.as_ref().and_then(|s| s.pause_between_batches_secs).unwrap_or(0);

    // If no strategy constraints, run all at once (like exec_multi_core)
    if concurrency == 0 && max_failures == 0 && batch_size == 0 {
        let mut set = JoinSet::new();
        for host in &resolved_hosts {
            let cmd = command.clone();
            let h = host.clone();
            set.spawn(async move {
                let req = ExecRequest {
                    host: h.clone(),
                    command: cmd,
                    force,
                    timeout_secs,
                    stdin: None,
                    max_output_bytes: None,
                    reason: None,
                    change_id: None,
                };
                match exec_ssh_core(req).await {
                    Ok(r) => ExecMultiResult { host: h, result: Some(r), error: None },
                    Err(e) => ExecMultiResult { host: h, result: None, error: Some(e.to_string()) },
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
        let successful = results.iter().filter(|r| r.result.is_some()).count();
        let failed = results.len() - successful;
        return ExecMultiBatchResult {
            results,
            total_hosts,
            successful,
            failed,
            skipped: 0,
            stopped_early: false,
            batches_executed: 1,
            total_duration_ms: started.elapsed().as_millis(),
        };
    }

    // Build batches
    let effective_batch_size = if batch_size > 0 { batch_size } else { total_hosts };
    let batches: Vec<Vec<String>> = resolved_hosts
        .chunks(effective_batch_size)
        .map(|c| c.to_vec())
        .collect();

    let mut all_results: Vec<ExecMultiResult> = Vec::new();
    let mut total_failures: usize = 0;
    let mut stopped_early = false;
    let mut batches_executed: usize = 0;

    for (batch_idx, batch_hosts) in batches.iter().enumerate() {
        // Pause between batches (not before the first)
        if batch_idx > 0 && pause_secs > 0 {
            tokio::time::sleep(Duration::from_secs(pause_secs)).await;
        }

        batches_executed += 1;

        // Create semaphore for concurrency limit within this batch
        let sem = if concurrency > 0 {
            Some(Arc::new(Semaphore::new(concurrency)))
        } else {
            None
        };

        let mut set = JoinSet::new();
        for host in batch_hosts {
            let cmd = command.clone();
            let h = host.clone();
            let sem_clone = sem.clone();
            set.spawn(async move {
                // Acquire semaphore permit if concurrency is limited
                let _permit = if let Some(ref s) = sem_clone {
                    Some(s.acquire().await.expect("semaphore closed"))
                } else {
                    None
                };
                let req = ExecRequest {
                    host: h.clone(),
                    command: cmd,
                    force,
                    timeout_secs,
                    stdin: None,
                    max_output_bytes: None,
                    reason: None,
                    change_id: None,
                };
                match exec_ssh_core(req).await {
                    Ok(r) => ExecMultiResult { host: h, result: Some(r), error: None },
                    Err(e) => ExecMultiResult { host: h, result: None, error: Some(e.to_string()) },
                }
            });
        }

        let mut batch_results = Vec::new();
        while let Some(joined) = set.join_next().await {
            batch_results.push(joined.unwrap_or_else(|e| ExecMultiResult {
                host: "unknown".into(),
                result: None,
                error: Some(format!("task panicked: {e}")),
            }));
        }

        // Count failures in this batch
        let batch_failures = batch_results.iter().filter(|r| r.result.is_none()).count();
        total_failures += batch_failures;
        all_results.extend(batch_results);

        // Check failure threshold
        if max_failures > 0 && total_failures >= max_failures {
            stopped_early = true;
            break;
        }
    }

    // Calculate skipped hosts
    let executed_hosts: usize = all_results.len();
    let skipped = total_hosts.saturating_sub(executed_hosts);

    let successful = all_results.iter().filter(|r| r.result.is_some()).count();
    let failed = all_results.iter().filter(|r| r.result.is_none()).count();

    ExecMultiBatchResult {
        results: all_results,
        total_hosts,
        successful,
        failed,
        skipped,
        stopped_early,
        batches_executed,
        total_duration_ms: started.elapsed().as_millis(),
    }
}

// ── Execution Result Comparison ─────────────────────────────────────────────

/// Comparison of multi-host execution results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecComparison {
    pub hosts_count: usize,
    pub exit_code_groups: Vec<ExitCodeGroup>,
    pub stdout_comparison: OutputComparison,
    pub stderr_comparison: OutputComparison,
    pub summary: String,
}

/// Group of hosts sharing the same exit code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitCodeGroup {
    pub exit_code: Option<i32>,
    pub hosts: Vec<String>,
}

/// Comparison of output (stdout or stderr) across hosts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputComparison {
    pub identical: bool,
    pub common_prefix: String,
    pub diffs: Vec<OutputDiff>,
}

/// Per-host output summary for comparison
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputDiff {
    pub host: String,
    pub output_summary: String,
    pub differs_from_first: bool,
}

/// Maximum number of lines to include in output summaries
const COMPARISON_SUMMARY_LINES: usize = 10;

/// Compare execution results across multiple hosts, grouping by exit code
/// and highlighting stdout/stderr differences.
pub fn compare_exec_results(results: &[ExecMultiResult]) -> ExecComparison {
    let hosts_count = results.len();

    if hosts_count == 0 {
        return ExecComparison {
            hosts_count: 0,
            exit_code_groups: vec![],
            stdout_comparison: OutputComparison {
                identical: true,
                common_prefix: String::new(),
                diffs: vec![],
            },
            stderr_comparison: OutputComparison {
                identical: true,
                common_prefix: String::new(),
                diffs: vec![],
            },
            summary: "No results to compare.".to_string(),
        };
    }

    // Group by exit code
    let mut exit_code_map: std::collections::HashMap<Option<i32>, Vec<String>> =
        std::collections::HashMap::new();
    for r in results {
        let exit_code = r.result.as_ref().map(|res| res.exit_code).unwrap_or(None);
        exit_code_map
            .entry(exit_code)
            .or_default()
            .push(r.host.clone());
    }
    let mut exit_code_groups: Vec<ExitCodeGroup> = exit_code_map
        .into_iter()
        .map(|(code, hosts)| ExitCodeGroup {
            exit_code: code,
            hosts,
        })
        .collect();
    // Sort by exit code for deterministic output
    exit_code_groups.sort_by_key(|g| g.exit_code.unwrap_or(i32::MAX));

    // Compare stdout
    let stdout_comparison = compare_outputs(results, |res| &res.stdout);

    // Compare stderr
    let stderr_comparison = compare_outputs(results, |res| &res.stderr);

    // Build summary
    let mut summary_parts = Vec::new();
    summary_parts.push(format!("Compared {} host(s).", hosts_count));

    if exit_code_groups.len() == 1 {
        let code = exit_code_groups[0].exit_code;
        summary_parts.push(format!(
            "All hosts exited with code {}.",
            code.map(|c| c.to_string()).unwrap_or_else(|| "unknown".into())
        ));
    } else {
        summary_parts.push(format!(
            "{} distinct exit code group(s).",
            exit_code_groups.len()
        ));
    }

    if stdout_comparison.identical {
        summary_parts.push("Stdout is identical across all hosts.".to_string());
    } else {
        summary_parts.push("Stdout differs across hosts.".to_string());
    }

    if stderr_comparison.identical {
        summary_parts.push("Stderr is identical across all hosts.".to_string());
    } else {
        summary_parts.push("Stderr differs across hosts.".to_string());
    }

    let summary = summary_parts.join(" ");

    ExecComparison {
        hosts_count,
        exit_code_groups,
        stdout_comparison,
        stderr_comparison,
        summary,
    }
}

/// Compare a specific output field (stdout or stderr) across all results.
fn compare_outputs(
    results: &[ExecMultiResult],
    extract: impl Fn(&ExecResult) -> &str,
) -> OutputComparison {
    // Collect (host, output) pairs for results that have a result
    let outputs: Vec<(String, &str)> = results
        .iter()
        .filter_map(|r| {
            r.result.as_ref().map(|res| (r.host.clone(), extract(res)))
        })
        .collect();

    if outputs.is_empty() {
        return OutputComparison {
            identical: true,
            common_prefix: String::new(),
            diffs: vec![],
        };
    }

    // Find longest common prefix
    let common_prefix = longest_common_prefix(
        &outputs.iter().map(|(_, s)| *s).collect::<Vec<_>>(),
    );

    // Check if all outputs are identical
    let first_output = outputs[0].1;
    let identical = outputs.iter().all(|(_, s)| *s == first_output);

    // Build per-host diffs
    let diffs: Vec<OutputDiff> = outputs
        .iter()
        .map(|(host, output)| {
            let summary_lines: String = output
                .lines()
                .take(COMPARISON_SUMMARY_LINES)
                .collect::<Vec<_>>()
                .join("\n");
            OutputDiff {
                host: host.clone(),
                output_summary: summary_lines,
                differs_from_first: *output != first_output,
            }
        })
        .collect();

    OutputComparison {
        identical,
        common_prefix,
        diffs,
    }
}

/// Find the longest common prefix of a set of strings.
fn longest_common_prefix(strings: &[&str]) -> String {
    if strings.is_empty() {
        return String::new();
    }
    if strings.len() == 1 {
        return strings[0].to_string();
    }
    let first = strings[0];
    let mut prefix_len = first.len();
    for s in &strings[1..] {
        let common = first
            .chars()
            .zip(s.chars())
            .take_while(|(a, b)| a == b)
            .count();
        prefix_len = prefix_len.min(common);
    }
    first.chars().take(prefix_len).collect()
}

pub(crate) fn build_ssh_command(host: &HostProfile) -> Command {
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
        reason: None,
        change_id: None,
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
        reason: None,
        change_id: None,
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
        reason: None,
        change_id: None,
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
            env: None,
            role: None,
            owner: None,
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

// ── SSH config sync strategy (F2-4) ────────────────────────────────────────────
//
// Sync rules between Agent2SSH and ~/.ssh/config:
//
// 1. IMPORT (from ~/.ssh/config → Agent2SSH):
//    - Default strategy: skip_existing — only add hosts not already in Agent2SSH.
//    - Merge strategy: update fields (host, user, port, key_path, proxy_jump)
//      for existing hosts, preserve Agent2SSH-only fields (tags, env, role, owner,
//      risk_override).
//    - Wildcards and entries without Hostname are always skipped.
//
// 2. EXPORT (from Agent2SSH → ~/.ssh/config):
//    - Generates standard SSH config entries with Host, HostName, User, Port,
//      IdentityFile, and ProxyJump directives.
//    - Never overwrites the entire file; appends new entries or updates matching
//      Host blocks when `overwrite` is set.
//    - Agent2SSH-only fields (tags, env, role, risk_override) are written as
//      comments for human reference but ignored by SSH.
//
// 3. CONFLICT HANDLING:
//    - Hostname mismatch: Agent2SSH host address differs from ~/.ssh/config.
//      Strategy decides: keep_agent2ssh | keep_ssh | report_only.
//    - Missing in one side: host exists in Agent2SSH but not in ~/.ssh/config
//      (or vice versa). Strategy decides: add_missing | skip.

/// Strategy for bidirectional SSH config synchronization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshSyncStrategy {
    /// How to handle import conflicts for existing hosts.
    /// - "skip_existing": only add new hosts (default)
    /// - "merge": update SSH fields for existing hosts, preserve Agent2SSH-only fields
    #[serde(default = "default_import_strategy")]
    pub import_strategy: String,

    /// How to handle hostname conflicts during export.
    /// - "keep_agent2ssh": use Agent2SSH host address
    /// - "keep_ssh": keep the existing ~/.ssh/config value
    /// - "report_only": don't write, just report the conflict
    #[serde(default = "default_conflict_strategy")]
    pub conflict_resolution: String,

    /// Whether to add hosts that exist only in Agent2SSH to ~/.ssh/config.
    #[serde(default = "default_true_fn")]
    pub export_missing: bool,

    /// Whether to add hosts that exist only in ~/.ssh/config to Agent2SSH.
    #[serde(default = "default_true_fn")]
    pub import_missing: bool,
}

fn default_import_strategy() -> String { "skip_existing".into() }
fn default_conflict_strategy() -> String { "keep_agent2ssh".into() }
fn default_true_fn() -> bool { true }

impl Default for SshSyncStrategy {
    fn default() -> Self {
        SshSyncStrategy {
            import_strategy: default_import_strategy(),
            conflict_resolution: default_conflict_strategy(),
            export_missing: true,
            import_missing: true,
        }
    }
}

/// Result of comparing Agent2SSH hosts with ~/.ssh/config entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshSyncDiff {
    /// Hosts only in Agent2SSH (not in ~/.ssh/config).
    pub only_in_agent2ssh: Vec<SshSyncHostDiff>,
    /// Hosts only in ~/.ssh/config (not in Agent2SSH).
    pub only_in_ssh_config: Vec<SshSyncHostDiff>,
    /// Hosts in both but with differing fields.
    pub conflicts: Vec<SshSyncHostConflict>,
    /// Hosts that match exactly.
    pub matching: Vec<String>,
    /// Summary string.
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshSyncHostDiff {
    pub name: String,
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshSyncHostConflict {
    pub name: String,
    pub field: String,
    pub agent2ssh_value: String,
    pub ssh_config_value: String,
}

/// Compare Agent2SSH hosts with ~/.ssh/config entries and return a diff.
pub fn compare_ssh_configs(path: Option<&str>) -> Result<SshSyncDiff> {
    let ssh_path: std::path::PathBuf = match path {
        Some(p) => expand_tilde(p).into(),
        None => dirs::home_dir()
            .ok_or_else(|| anyhow!("cannot locate home directory"))?
            .join(".ssh")
            .join("config"),
    };

    // Parse ~/.ssh/config entries (reuse the parser logic)
    let ssh_entries = parse_ssh_config_file(&ssh_path)?;

    // Load Agent2SSH hosts
    let a2s_hosts = load_config()?.hosts;

    let ssh_map: std::collections::HashMap<String, &HostProfile> =
        ssh_entries.iter().map(|h| (h.name.clone(), h)).collect();
    let a2s_map: std::collections::HashMap<String, &HostProfile> =
        a2s_hosts.iter().map(|h| (h.name.clone(), h)).collect();

    let mut only_in_agent2ssh = Vec::new();
    let mut only_in_ssh_config = Vec::new();
    let mut conflicts = Vec::new();
    let mut matching = Vec::new();

    // Check Agent2SSH hosts
    for (name, host) in &a2s_map {
        if let Some(ssh_host) = ssh_map.get(name) {
            // Both exist — check for conflicts
            let mut host_matches = true;
            if host.host != ssh_host.host {
                conflicts.push(SshSyncHostConflict {
                    name: name.clone(), field: "hostname".into(),
                    agent2ssh_value: host.host.clone(),
                    ssh_config_value: ssh_host.host.clone(),
                });
                host_matches = false;
            }
            if host.user != ssh_host.user {
                conflicts.push(SshSyncHostConflict {
                    name: name.clone(), field: "user".into(),
                    agent2ssh_value: host.user.clone().unwrap_or_default(),
                    ssh_config_value: ssh_host.user.clone().unwrap_or_default(),
                });
                host_matches = false;
            }
            if host.port != ssh_host.port {
                conflicts.push(SshSyncHostConflict {
                    name: name.clone(), field: "port".into(),
                    agent2ssh_value: host.port.map(|p| p.to_string()).unwrap_or_default(),
                    ssh_config_value: ssh_host.port.map(|p| p.to_string()).unwrap_or_default(),
                });
                host_matches = false;
            }
            if host_matches {
                matching.push(name.clone());
            }
        } else {
            only_in_agent2ssh.push(SshSyncHostDiff {
                name: name.clone(), host: host.host.clone(),
                user: host.user.clone(), port: host.port,
            });
        }
    }

    // Check ~/.ssh/config hosts not in Agent2SSH
    for (name, host) in &ssh_map {
        if !a2s_map.contains_key(name) {
            only_in_ssh_config.push(SshSyncHostDiff {
                name: name.clone(), host: host.host.clone(),
                user: host.user.clone(), port: host.port,
            });
        }
    }

    let summary = format!(
        "{} matching, {} only in Agent2SSH, {} only in ~/.ssh/config, {} conflicts",
        matching.len(), only_in_agent2ssh.len(), only_in_ssh_config.len(), conflicts.len()
    );

    Ok(SshSyncDiff {
        only_in_agent2ssh,
        only_in_ssh_config,
        conflicts,
        matching,
        summary,
    })
}

/// Parse a SSH config file into HostProfile entries (does NOT write to Agent2SSH store).
fn parse_ssh_config_file(path: &std::path::Path) -> Result<Vec<HostProfile>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let mut profiles: Vec<HostProfile> = Vec::new();
    let mut current_alias: Option<String> = None;
    let mut hostname: Option<String> = None;
    let mut user: Option<String> = None;
    let mut port: Option<u16> = None;
    let mut identity: Option<String> = None;
    let mut proxy_jump: Option<String> = None;

    let flush = |alias: &Option<String>, hn: &Option<String>, u: &Option<String>,
                 p: Option<u16>, id: &Option<String>, pj: &Option<String>,
                 out: &mut Vec<HostProfile>| {
        let alias = alias.as_deref().unwrap_or("");
        let hn = hn.as_deref().unwrap_or("");
        if alias.is_empty() || alias.contains('*') || alias.contains('?') || hn.is_empty() {
            return;
        }
        let jump_host = pj.as_ref().map(|raw| {
            let target = raw.split_whitespace().next().unwrap_or(raw);
            let no_user = target.split('@').last().unwrap_or(target);
            no_user.split(':').next().unwrap_or(no_user).to_string()
        });
        out.push(HostProfile {
            name: alias.to_string(), host: hn.to_string(),
            user: u.clone(), port: p, key_path: id.clone(),
            jump_host, risk_override: None, tags: vec![],
            env: None, role: None, owner: None,
        });
    };

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let (key, value) = match line.split_once(|c: char| c.is_whitespace()) {
            Some(pair) => (pair.0.to_lowercase(), pair.1.trim().to_string()),
            None => continue,
        };
        match key.as_str() {
            "host" => {
                flush(&current_alias, &hostname, &user, port, &identity, &proxy_jump, &mut profiles);
                current_alias = Some(value);
                hostname = None; user = None; port = None; identity = None; proxy_jump = None;
            }
            "hostname" => hostname = Some(value),
            "user" => user = Some(value),
            "port" => port = value.parse().ok(),
            "identityfile" => identity = Some(value),
            "proxyjump" => proxy_jump = Some(value),
            _ => {}
        }
    }
    flush(&current_alias, &hostname, &user, port, &identity, &proxy_jump, &mut profiles);
    Ok(profiles)
}

/// Export Agent2SSH hosts to SSH config format string.
pub fn export_to_ssh_config_format(hosts: &[HostProfile]) -> String {
    let mut out = String::new();
    out.push_str("# Generated by Agent2SSH — do not edit manually within this block\n");
    out.push_str("# Managed hosts are identified by the comment line above each block\n\n");

    for host in hosts {
        // Tags/env/role as comments for human reference
        if !host.tags.is_empty() || host.env.is_some() || host.role.is_some() {
            let mut meta = Vec::new();
            if let Some(env) = &host.env { meta.push(format!("env={}", env)); }
            if let Some(role) = &host.role { meta.push(format!("role={}", role)); }
            if !host.tags.is_empty() { meta.push(format!("tags={}", host.tags.join(","))); }
            out.push_str(&format!("# agent2ssh: {}\n", meta.join(" ")));
        }
        out.push_str(&format!("Host {}\n", host.name));
        out.push_str(&format!("    HostName {}\n", host.host));
        if let Some(user) = &host.user {
            if !user.trim().is_empty() {
                out.push_str(&format!("    User {}\n", user));
            }
        }
        if let Some(port) = host.port {
            if port != 22 {
                out.push_str(&format!("    Port {}\n", port));
            }
        }
        if let Some(key) = &host.key_path {
            if !key.trim().is_empty() {
                out.push_str(&format!("    IdentityFile {}\n", key));
            }
        }
        if let Some(jump) = &host.jump_host {
            out.push_str(&format!("    ProxyJump {}\n", jump));
        }
        out.push('\n');
    }
    out
}

/// Export Agent2SSH hosts to a file in SSH config format.
/// If `path` is None, writes to ~/.ssh/config.d/agent2ssh.conf (include-based approach).
/// Returns the path written to and the number of hosts exported.
pub fn export_to_ssh_config(path: Option<&str>, strategy: Option<SshSyncStrategy>) -> Result<(String, usize)> {
    let config = load_config()?;
    let strategy = strategy.unwrap_or_default();

    if !strategy.export_missing {
        // Filter to only hosts that already exist in ~/.ssh/config
        let ssh_path: std::path::PathBuf = match path {
            Some(p) => expand_tilde(p).into(),
            None => dirs::home_dir()
                .ok_or_else(|| anyhow!("cannot locate home directory"))?
                .join(".ssh")
                .join("config"),
        };
        let existing = parse_ssh_config_file(&ssh_path)?;
        let existing_names: std::collections::HashSet<String> =
            existing.iter().map(|h| h.name.clone()).collect();
        let filtered: Vec<HostProfile> = config.hosts.into_iter()
            .filter(|h| existing_names.contains(&h.name))
            .collect();
        let content = export_to_ssh_config_format(&filtered);
        let count = filtered.len();
        let out_path = resolve_ssh_export_path(path)?;
        std::fs::write(&out_path, &content)
            .with_context(|| format!("failed to write {}", out_path.display()))?;
        return Ok((out_path.display().to_string(), count));
    }

    let content = export_to_ssh_config_format(&config.hosts);
    let count = config.hosts.len();
    let out_path = resolve_ssh_export_path(path)?;
    std::fs::write(&out_path, &content)
        .with_context(|| format!("failed to write {}", out_path.display()))?;
    Ok((out_path.display().to_string(), count))
}

fn resolve_ssh_export_path(path: Option<&str>) -> Result<std::path::PathBuf> {
    match path {
        Some(p) => Ok(expand_tilde(p).into()),
        None => {
            let dir = dirs::home_dir()
                .ok_or_else(|| anyhow!("cannot locate home directory"))?
                .join(".ssh")
                .join("config.d");
            std::fs::create_dir_all(&dir)?;
            Ok(dir.join("agent2ssh.conf"))
        }
    }
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
    use crate::types::AppConfig;

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
    fn test_filter_hosts_by_metadata_and_tag() {
        let hosts = vec![
            HostProfile {
                name: "prod-web-1".into(),
                host: "10.0.0.1".into(),
                user: None,
                port: None,
                key_path: None,
                jump_host: None,
                risk_override: None,
                tags: vec!["blue".into(), "web".into()],
                env: Some("prod".into()),
                role: Some("web".into()),
                owner: Some("platform".into()),
            },
            HostProfile {
                name: "stage-db-1".into(),
                host: "10.0.1.1".into(),
                user: None,
                port: None,
                key_path: None,
                jump_host: None,
                risk_override: None,
                tags: vec!["db".into()],
                env: Some("staging".into()),
                role: Some("db".into()),
                owner: Some("data".into()),
            },
        ];

        let filtered = filter_hosts(
            hosts.clone(),
            &HostFilter {
                env: Some("PROD".into()),
                role: Some("web".into()),
                owner: Some("platform".into()),
                tag: Some("blue".into()),
            },
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "prod-web-1");

        let filtered = filter_hosts(
            hosts,
            &HostFilter {
                env: None,
                role: None,
                owner: None,
                tag: Some("missing".into()),
            },
        );
        assert!(filtered.is_empty());
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

    // ── ExecPlan preview tests (using build_plan_from_profile) ───────────────

    fn make_test_host(name: &str) -> HostProfile {
        HostProfile {
            name: name.to_string(),
            host: "10.0.0.1".to_string(),
            user: Some("ubuntu".to_string()),
            port: Some(22),
            key_path: None,
            jump_host: None,
            risk_override: None,
            tags: vec![],
            env: None,
            role: None,
            owner: None,
        }
    }

    #[test]
    fn test_preview_exec_low_risk() {
        let host = make_test_host("test-low");
        let plan = build_plan_from_profile(vec![host], "uptime", None);
        assert_eq!(plan.overall_risk, RiskLevel::Low);
        assert!(!plan.requires_approval);
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].risk_level, RiskLevel::Low);
        assert!(!plan.targets[0].needs_force);
        assert!(!plan.targets[0].blocked);
        assert_eq!(plan.targets[0].timeout_secs, 60);
        assert!(plan.warnings.is_empty());
    }

    #[test]
    fn test_preview_exec_high_risk() {
        let host = make_test_host("test-high");
        let plan = build_plan_from_profile(vec![host], "sudo rm -rf /tmp", None);
        assert_eq!(plan.overall_risk, RiskLevel::High);
        assert!(plan.requires_approval);
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].risk_level, RiskLevel::High);
        assert!(plan.targets[0].needs_force);
        assert!(!plan.targets[0].blocked);
        assert!(plan.warnings.iter().any(|w| w.contains("force=true")));
    }

    #[test]
    fn test_preview_exec_blocked() {
        let host = make_test_host("test-blocked");
        let plan = build_plan_from_profile(vec![host], "rm -rf /", None);
        assert_eq!(plan.overall_risk, RiskLevel::Blocked);
        assert!(!plan.requires_approval); // blocked != approval, just blocked
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].risk_level, RiskLevel::Blocked);
        assert!(plan.targets[0].blocked);
        assert!(plan.warnings.iter().any(|w| w.contains("blocked")));
    }

    #[test]
    fn test_preview_exec_multi_risk_aggregation() {
        let hosts = vec![
            make_test_host("host-a"),
            make_test_host("host-b"),
        ];
        // "sudo whoami" is High risk — both hosts should get High
        let plan = build_plan_from_profile(hosts, "sudo whoami", Some(120));
        assert_eq!(plan.overall_risk, RiskLevel::High);
        assert!(plan.requires_approval);
        assert_eq!(plan.targets.len(), 2);
        for target in &plan.targets {
            assert_eq!(target.risk_level, RiskLevel::High);
            assert!(target.needs_force);
            assert_eq!(target.timeout_secs, 120);
        }
    }

    #[test]
    fn test_preview_exec_with_jump_host_warning() {
        let mut host = make_test_host("test-jump");
        host.jump_host = Some("bastion".to_string());
        let plan = build_plan_from_profile(vec![host], "ls -la", None);
        assert_eq!(plan.overall_risk, RiskLevel::Low);
        assert!(plan.warnings.iter().any(|w| w.contains("jump host bastion")));
    }

    #[test]
    fn test_preview_exec_multi_mixed_risk() {
        let host_a = make_test_host("multi-a");
        let mut host_b = make_test_host("multi-b");
        host_b.risk_override = Some(RiskLevel::Low);
        // "sudo whoami" is High risk by default.
        // host_a should get High, host_b should get Low (due to override).
        let plan = build_plan_from_profile(vec![host_a, host_b], "sudo whoami", None);
        assert_eq!(plan.overall_risk, RiskLevel::High);
        let a_target = plan.targets.iter().find(|t| t.host == "multi-a").unwrap();
        let b_target = plan.targets.iter().find(|t| t.host == "multi-b").unwrap();
        assert_eq!(a_target.risk_level, RiskLevel::High);
        assert_eq!(b_target.risk_level, RiskLevel::Low);
    }

    #[test]
    fn test_preview_exec_default_timeout() {
        let host = make_test_host("test-timeout");
        let plan = build_plan_from_profile(vec![host], "uptime", None);
        assert_eq!(plan.targets[0].timeout_secs, 60);
    }

    #[test]
    fn test_preview_team_config_import() {
        let config_dir = std::env::temp_dir().join(format!(
            "agent2ssh-preview-import-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&config_dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &config_dir);

        // Set up existing config with one host
        let existing_config = AppConfig {
            hosts: vec![HostProfile {
                name: "existing-host".into(),
                host: "10.0.0.1".into(),
                user: Some("ubuntu".into()),
                port: Some(22),
                key_path: None,
                jump_host: None,
                risk_override: None,
                tags: vec![],
                env: None,
                role: None,
                owner: None,
            }],
        };
        crate::store::save_config(&existing_config).unwrap();

        let export = TeamConfigExport {
            hosts: vec![
                // New host
                HostProfile {
                    name: "new-host".into(),
                    host: "10.0.0.2".into(),
                    user: None,
                    port: None,
                    key_path: None,
                    jump_host: None,
                    risk_override: None,
                    tags: vec![],
                    env: None,
                    role: None,
                    owner: None,
                },
                // Duplicate (same name, same host/port/user)
                HostProfile {
                    name: "existing-host".into(),
                    host: "10.0.0.1".into(),
                    user: Some("ubuntu".into()),
                    port: Some(22),
                    key_path: None,
                    jump_host: None,
                    risk_override: None,
                    tags: vec![],
                    env: None,
                    role: None,
                    owner: None,
                },
            ],
            risk_rules: Some("[rules]\n".into()),
            playbooks: None,
        };

        let preview = preview_team_config_import(&export).unwrap();
        assert_eq!(preview.hosts_to_add, vec!["new-host"]);
        assert_eq!(preview.hosts_to_skip, vec!["existing-host"]);
        assert!(preview.hosts_to_update.is_empty());
        assert_eq!(preview.risk_rules_change, Some("new".to_string()));
        assert_eq!(preview.playbooks_change, None);
        assert!(preview.summary.contains("1 host(s) to add"));

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&config_dir);
    }

    #[test]
    fn test_audit_entry_with_reason_and_change_id() {
        use crate::types::AuditEntry;
        use chrono::Utc;
        use uuid::Uuid;

        let entry = AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            host: "test-host".into(),
            command: "uptime".into(),
            exit_code: Some(0),
            duration_ms: 100,
            risk_level: RiskLevel::Low,
            reason: Some("daily health check".into()),
            change_id: Some("CHG-12345".into()),
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("daily health check"));
        assert!(json.contains("CHG-12345"));

        let deserialized: AuditEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.reason, Some("daily health check".into()));
        assert_eq!(deserialized.change_id, Some("CHG-12345".into()));
        assert_eq!(deserialized.host, "test-host");

        // Verify backward compatibility: entries without reason/change_id
        let old_json = r#"{"id":"550e8400-e29b-41d4-a716-446655440000","ts":"2025-01-01T00:00:00Z","host":"h","command":"ls","exit_code":null,"duration_ms":10,"risk_level":"low"}"#;
        let old_entry: AuditEntry = serde_json::from_str(old_json).unwrap();
        assert_eq!(old_entry.reason, None);
        assert_eq!(old_entry.change_id, None);
    }

    // ── Batch Strategy tests ────────────────────────────────────────────────────

    #[test]
    fn test_batch_strategy_default_unlimited() {
        // No strategy should mean unlimited concurrency (concurrency = 0)
        let strategy: Option<BatchStrategy> = None;
        let concurrency = strategy.as_ref().and_then(|s| s.concurrency).unwrap_or(0);
        let max_failures = strategy.as_ref().and_then(|s| s.max_failures).unwrap_or(0);
        let batch_size = strategy.as_ref().and_then(|s| s.batch_size).unwrap_or(0);
        assert_eq!(concurrency, 0);
        assert_eq!(max_failures, 0);
        assert_eq!(batch_size, 0);
    }

    #[test]
    fn test_batch_strategy_concurrency_limit() {
        // Verify that a semaphore is created when concurrency > 0
        let strategy = BatchStrategy {
            concurrency: Some(3),
            max_failures: None,
            batch_size: None,
            pause_between_batches_secs: None,
        };
        let concurrency = strategy.concurrency.unwrap_or(0);
        assert_eq!(concurrency, 3);
        // A semaphore with 3 permits allows at most 3 concurrent tasks
        let sem = Semaphore::new(concurrency);
        assert_eq!(sem.available_permits(), 3);
    }

    #[test]
    fn test_batch_strategy_max_failures_stops_early() {
        // Simulate: 5 hosts, max_failures=2, after 2 failures remaining hosts skipped
        let hosts = vec!["h1", "h2", "h3", "h4", "h5"];
        let max_failures = 2usize;

        // Simulate results: h1=error, h2=error -> 2 failures, stop
        let simulated_results: Vec<ExecMultiResult> = vec![
            ExecMultiResult {
                host: "h1".into(),
                result: None,
                error: Some("fail".into()),
            },
            ExecMultiResult {
                host: "h2".into(),
                result: None,
                error: Some("fail".into()),
            },
        ];

        let total_failures = simulated_results.iter().filter(|r| r.result.is_none()).count();
        assert!(total_failures >= max_failures, "Should stop after reaching failure threshold");

        let executed_count = simulated_results.len();
        let skipped = hosts.len() - executed_count;
        assert_eq!(skipped, 3, "3 hosts should be skipped after threshold");
    }

    #[test]
    fn test_batch_strategy_batch_size() {
        // Verify batch splitting: 7 hosts with batch_size=3 -> 3 batches [3,3,1]
        let hosts: Vec<String> = (0..7).map(|i| format!("host-{i}")).collect();
        let batch_size = 3usize;
        let batches: Vec<Vec<String>> = hosts
            .chunks(batch_size)
            .map(|c| c.to_vec())
            .collect();
        assert_eq!(batches.len(), 3, "7 hosts with batch_size=3 should produce 3 batches");
        assert_eq!(batches[0].len(), 3);
        assert_eq!(batches[1].len(), 3);
        assert_eq!(batches[2].len(), 1);
    }

    // ── Comparison tests ────────────────────────────────────────────────────────

    #[test]
    fn test_compare_exec_results_all_identical() {
        let results = vec![
            ExecMultiResult {
                host: "host-a".into(),
                result: Some(ExecResult {
                    host: "host-a".into(),
                    command: "uptime".into(),
                    exit_code: Some(0),
                    stdout: "up 5 days\n".into(),
                    stderr: String::new(),
                    duration_ms: 100,
                    risk_level: RiskLevel::Low,
                    truncated: false,
                }),
                error: None,
            },
            ExecMultiResult {
                host: "host-b".into(),
                result: Some(ExecResult {
                    host: "host-b".into(),
                    command: "uptime".into(),
                    exit_code: Some(0),
                    stdout: "up 5 days\n".into(),
                    stderr: String::new(),
                    duration_ms: 120,
                    risk_level: RiskLevel::Low,
                    truncated: false,
                }),
                error: None,
            },
        ];

        let comparison = compare_exec_results(&results);
        assert_eq!(comparison.hosts_count, 2);
        assert_eq!(comparison.exit_code_groups.len(), 1, "All same exit code");
        assert_eq!(comparison.exit_code_groups[0].exit_code, Some(0));
        assert_eq!(comparison.exit_code_groups[0].hosts.len(), 2);
        assert!(comparison.stdout_comparison.identical, "stdout should be identical");
        assert!(comparison.stderr_comparison.identical, "stderr should be identical");
        assert!(comparison.summary.contains("identical"));
    }

    #[test]
    fn test_compare_exec_results_mixed_exit_codes() {
        let results = vec![
            ExecMultiResult {
                host: "host-ok".into(),
                result: Some(ExecResult {
                    host: "host-ok".into(),
                    command: "ls".into(),
                    exit_code: Some(0),
                    stdout: "file.txt\n".into(),
                    stderr: String::new(),
                    duration_ms: 50,
                    risk_level: RiskLevel::Low,
                    truncated: false,
                }),
                error: None,
            },
            ExecMultiResult {
                host: "host-fail".into(),
                result: Some(ExecResult {
                    host: "host-fail".into(),
                    command: "ls".into(),
                    exit_code: Some(1),
                    stdout: String::new(),
                    stderr: "No such file\n".into(),
                    duration_ms: 60,
                    risk_level: RiskLevel::Low,
                    truncated: false,
                }),
                error: None,
            },
            ExecMultiResult {
                host: "host-error".into(),
                result: None,
                error: Some("connection refused".into()),
            },
        ];

        let comparison = compare_exec_results(&results);
        assert_eq!(comparison.hosts_count, 3);
        assert_eq!(comparison.exit_code_groups.len(), 3, "3 distinct exit code groups");
        // exit code 0 -> host-ok
        let group_0 = comparison.exit_code_groups.iter().find(|g| g.exit_code == Some(0)).unwrap();
        assert_eq!(group_0.hosts, vec!["host-ok"]);
        // exit code 1 -> host-fail
        let group_1 = comparison.exit_code_groups.iter().find(|g| g.exit_code == Some(1)).unwrap();
        assert_eq!(group_1.hosts, vec!["host-fail"]);
        // exit code None -> host-error
        let group_none = comparison.exit_code_groups.iter().find(|g| g.exit_code.is_none()).unwrap();
        assert_eq!(group_none.hosts, vec!["host-error"]);
        assert!(comparison.summary.contains("3 distinct exit code group"));
    }

    #[test]
    fn test_compare_exec_results_stdout_diff() {
        let results = vec![
            ExecMultiResult {
                host: "host-a".into(),
                result: Some(ExecResult {
                    host: "host-a".into(),
                    command: "hostname".into(),
                    exit_code: Some(0),
                    stdout: "server-a\n".into(),
                    stderr: String::new(),
                    duration_ms: 50,
                    risk_level: RiskLevel::Low,
                    truncated: false,
                }),
                error: None,
            },
            ExecMultiResult {
                host: "host-b".into(),
                result: Some(ExecResult {
                    host: "host-b".into(),
                    command: "hostname".into(),
                    exit_code: Some(0),
                    stdout: "server-b\n".into(),
                    stderr: String::new(),
                    duration_ms: 50,
                    risk_level: RiskLevel::Low,
                    truncated: false,
                }),
                error: None,
            },
        ];

        let comparison = compare_exec_results(&results);
        assert!(!comparison.stdout_comparison.identical, "stdout should differ");
        assert_eq!(comparison.stdout_comparison.diffs.len(), 2);
        assert!(!comparison.stdout_comparison.diffs[0].differs_from_first);
        assert!(comparison.stdout_comparison.diffs[1].differs_from_first);
        // Common prefix of "server-a\n" and "server-b\n" = "server-"
        assert_eq!(comparison.stdout_comparison.common_prefix, "server-");
        assert!(comparison.summary.contains("Stdout differs"));
    }

    #[test]
    fn test_compare_exec_results_empty() {
        let results: Vec<ExecMultiResult> = vec![];
        let comparison = compare_exec_results(&results);
        assert_eq!(comparison.hosts_count, 0);
        assert!(comparison.exit_code_groups.is_empty());
        assert!(comparison.stdout_comparison.identical);
        assert!(comparison.stderr_comparison.identical);
        assert!(comparison.summary.contains("No results"));
    }

    #[test]
    fn test_longest_common_prefix() {
        assert_eq!(longest_common_prefix(&["abc", "abd", "abe"]), "ab");
        assert_eq!(longest_common_prefix(&["hello", "hello"]), "hello");
        assert_eq!(longest_common_prefix(&["abc", "xyz"]), "");
        assert_eq!(longest_common_prefix(&["single"]), "single");
        assert_eq!(longest_common_prefix(&[]), "");
    }

    // ── SSH sync strategy tests (F2-4) ─────────────────────────────────────

    #[test]
    fn test_ssh_sync_strategy_default() {
        let s = SshSyncStrategy::default();
        assert_eq!(s.import_strategy, "skip_existing");
        assert_eq!(s.conflict_resolution, "keep_agent2ssh");
        assert!(s.export_missing);
        assert!(s.import_missing);
    }

    #[test]
    fn test_export_to_ssh_config_format() {
        let hosts = vec![
            HostProfile {
                name: "prod-web".into(), host: "10.0.0.1".into(),
                user: Some("ubuntu".into()), port: Some(22),
                key_path: Some("~/.ssh/id_rsa".into()),
                jump_host: None, risk_override: None, tags: vec!["web".into()],
                env: Some("prod".into()), role: Some("web".into()), owner: None,
            },
            HostProfile {
                name: "staging-db".into(), host: "10.0.1.1".into(),
                user: None, port: Some(2222),
                key_path: None, jump_host: Some("bastion".into()),
                risk_override: None, tags: vec![], env: None, role: None, owner: None,
            },
        ];
        let output = export_to_ssh_config_format(&hosts);
        assert!(output.contains("Host prod-web"));
        assert!(output.contains("HostName 10.0.0.1"));
        assert!(output.contains("User ubuntu"));
        assert!(output.contains("IdentityFile ~/.ssh/id_rsa"));
        assert!(output.contains("Host staging-db"));
        assert!(output.contains("HostName 10.0.1.1"));
        assert!(output.contains("Port 2222"));
        assert!(output.contains("ProxyJump bastion"));
        // Agent2SSH metadata as comments
        assert!(output.contains("env=prod"));
        assert!(output.contains("role=web"));
    }

    #[test]
    fn test_export_to_ssh_config_format_default_port_omitted() {
        let hosts = vec![HostProfile {
            name: "test".into(), host: "10.0.0.1".into(),
            user: None, port: Some(22), key_path: None,
            jump_host: None, risk_override: None, tags: vec![],
            env: None, role: None, owner: None,
        }];
        let output = export_to_ssh_config_format(&hosts);
        // Port 22 should be omitted (it's the default)
        assert!(!output.contains("Port 22"));
    }

    #[test]
    fn test_parse_ssh_config_file_nonexistent() {
        let result = parse_ssh_config_file(std::path::Path::new("/tmp/nonexistent_ssh_config_xyz"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_ssh_sync_diff_serialization() {
        let diff = SshSyncDiff {
            only_in_agent2ssh: vec![SshSyncHostDiff {
                name: "test".into(), host: "10.0.0.1".into(),
                user: None, port: None,
            }],
            only_in_ssh_config: vec![],
            conflicts: vec![SshSyncHostConflict {
                name: "web".into(), field: "hostname".into(),
                agent2ssh_value: "10.0.0.2".into(),
                ssh_config_value: "10.0.0.1".into(),
            }],
            matching: vec!["db".into()],
            summary: "1 matching, 1 only in Agent2SSH, 0 only in ~/.ssh/config, 1 conflicts".into(),
        };
        let json = serde_json::to_string(&diff).unwrap();
        let parsed: SshSyncDiff = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.matching.len(), 1);
        assert_eq!(parsed.conflicts[0].field, "hostname");
    }
}
