use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    store::{config_dir, load_config, save_config_unlocked, store_write_lock},
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
