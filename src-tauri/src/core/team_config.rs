use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    store::{config_dir, load_config, save_config_unlocked, store_write_lock, write_private_text},
    types::HostProfile,
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TeamConfigExport {
    /// Host profiles with key_path/password/passphrase stripped for safe sharing
    pub hosts: Vec<HostProfile>,
    /// Raw TOML content of risk_rules.toml (if it exists)
    pub risk_rules: Option<String>,
    /// Raw TOML content of playbooks.toml (if it exists)
    pub playbooks: Option<String>,
}

/// Export team configuration: hosts (without private key paths), risk rules,
/// and playbooks. Suitable for sharing within a team.
///
/// The risk-rules half is still read from `risk_rules.toml` only. When a
/// `policy.toml` exists, `load_risk_rules` never opens that file — it reads
/// `[risk.*]` out of the policy instead — so this export reports "not
/// configured" for a machine whose risk rules are very much in force, and the
/// import that follows overwrites a legacy file nothing reads. Tracked as
/// L2-108 in `docs/reports/OPT-LOOP-L2-PENDING.md`, which fixes the target
/// file (the policy, via `existing_policy_path()`) and leaves one question
/// open: whether an import replaces the whole `[risk.*]` section or merges
/// into it. "When the export carries no `risk_rules`" is *not* open — the
/// import already answers it, by touching nothing: every branch below is
/// inside `if let Some(...)`, so an export without that half neither rewrites
/// nor clears the local section.
pub fn export_team_config() -> Result<TeamConfigExport> {
    let config = load_config()?;

    // Strip key_path from all hosts
    let hosts: Vec<HostProfile> = config
        .hosts
        .into_iter()
        .map(|mut h| {
            h.key_path = None;
            h.password = None;
            h.passphrase = None;
            h
        })
        .collect();

    let config_dir = config_dir()?;

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
///
/// "Optionally" is decided by the export, not by the caller: a missing
/// `risk_rules` / `playbooks` key in the payload leaves the local file alone,
/// and `ImportResult` reports the difference through `risk_rules_imported` /
/// `playbooks_imported`. An import that carries `None` is therefore a no-op for
/// that half rather than a clear — which is the behaviour L2-108's fix has to
/// preserve when it moves the target from `risk_rules.toml` to the policy file.
pub fn import_team_config(export: &TeamConfigExport) -> Result<ImportResult> {
    let _guard = store_write_lock()?;
    let mut config = load_config()?;

    let mut added = 0u32;
    let mut skipped = 0u32;
    let mut updated = 0u32;
    for host in &export.hosts {
        if let Some(existing) = config
            .hosts
            .iter_mut()
            .find(|existing| existing.name == host.name)
        {
            if team_host_same(existing, host) {
                skipped += 1;
            } else {
                let key_path = existing.key_path.clone();
                let password = existing.password.clone();
                let passphrase = existing.passphrase.clone();
                let init_command = existing.init_command.clone();
                let mut next = host.clone();
                if next.key_path.is_none() {
                    next.key_path = key_path;
                }
                if next.password.is_none() {
                    next.password = password;
                }
                if next.passphrase.is_none() {
                    next.passphrase = passphrase;
                }
                // `export_team_config` never strips `init_command`, so an
                // omitted one means "the bundle came from a machine that did
                // not have it", never "the sharer turned it off". Without this
                // an import turned a working post-connect command off with no
                // report to say so: `team_host_same` does not compare the
                // field either, so the same import reported the host as
                // skipped and rewrote it. Deliberately not in
                // `team_host_same`'s eleven -- doing that would report a
                // host as updated on a difference it then preserves.
                if next.init_command.is_none() {
                    next.init_command = init_command;
                }
                *existing = next;
                updated += 1;
            }
        } else {
            config.hosts.push(host.clone());
            added += 1;
        }
    }
    config.hosts.sort_by(|a, b| a.name.cmp(&b.name));
    save_config_unlocked(&config)?;

    let config_dir = config_dir()?;

    if let Some(ref rules) = export.risk_rules {
        let path = config_dir.join("risk_rules.toml");
        write_private_text(&path, rules)
            .with_context(|| format!("failed to write {}", path.display()))?;
    }

    if let Some(ref pbs) = export.playbooks {
        let path = config_dir.join("playbooks.toml");
        write_private_text(&path, pbs)
            .with_context(|| format!("failed to write {}", path.display()))?;
    }

    Ok(ImportResult {
        hosts_added: added,
        hosts_skipped: skipped,
        hosts_updated: updated,
        risk_rules_imported: export.risk_rules.is_some(),
        playbooks_imported: export.playbooks.is_some(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub hosts_added: u32,
    pub hosts_skipped: u32,
    pub hosts_updated: u32,
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
    /// Names of hosts that will be updated: the name matches and at least one
    /// of the eleven fields [`team_host_same`] compares differs — `host`,
    /// `user`, `port`, `jump_host`, `proxy_id`, `risk_override`, `tags`,
    /// `group`, `env`, `role`, `owner`. A same-name host that matches on all
    /// eleven is reported as skipped, not updated.
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
    let config_dir = config_dir()?;

    let existing_map: std::collections::HashMap<String, &HostProfile> =
        config.hosts.iter().map(|h| (h.name.clone(), h)).collect();

    let mut hosts_to_add = Vec::new();
    let mut hosts_to_skip = Vec::new();
    let mut hosts_to_update = Vec::new();

    for host in &export.hosts {
        match existing_map.get(&host.name) {
            Some(existing) => {
                if team_host_same(existing, host) {
                    hosts_to_skip.push(host.name.clone());
                } else {
                    hosts_to_update.push(host.name.clone());
                }
            }
            None => hosts_to_add.push(host.name.clone()),
        }
    }

    let risk_rules_change = if export.risk_rules.is_some() {
        if config_dir.join("risk_rules.toml").exists() {
            Some("overwrite".to_string())
        } else {
            Some("new".to_string())
        }
    } else {
        None
    };
    let playbooks_change = if export.playbooks.is_some() {
        if config_dir.join("playbooks.toml").exists() {
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

/// Whether an incoming team-config host matches the stored one closely enough
/// that the import can skip it.
///
/// Compares eleven fields. `init_command` and `passphrase` are deliberately
/// outside the set, because both are *preserved* rather than compared when a
/// host is updated (`import_team_config`): a field that reports a difference
/// would then be a field the import does not act on, so a host differing only
/// in one of them is reported as skipped -- correctly, since nothing about it
/// changes. `password` and `key_path` are outside for the same reason.
fn team_host_same(existing: &HostProfile, incoming: &HostProfile) -> bool {
    existing.host == incoming.host
        && existing.user == incoming.user
        && existing.port == incoming.port
        && existing.jump_host == incoming.jump_host
        && existing.proxy_id == incoming.proxy_id
        && existing.risk_override == incoming.risk_override
        && existing.tags == incoming.tags
        && existing.group == incoming.group
        && existing.env == incoming.env
        && existing.role == incoming.role
        && existing.owner == incoming.owner
}
