//! CNB #38: explicit, audited retag of legacy credential entries and bare
//! references.
//!
//! Phase 2 gave new writes content tags and `$agent2ssh-secret$:<tag>`
//! references, but credentials stored before the upgrade keep an empty tag and
//! a bare `$agent2ssh-secret$` reference forever: `entry_tags()` drops tags
//! that fail `is_valid_tag()`, so an ordinary save has nothing to upgrade a
//! bare reference with (pinned by
//! `tests/store_phase2.rs::a_fully_legacy_pair_keeps_its_bare_reference_on_the_next_save`).
//! A bare reference only warns on the four restore paths, so a credential
//! changed behind the config can still be silently picked.
//!
//! This module is the explicit way out, in the style of `config_repair`
//! (#26 phase D): nothing happens without the CLI's `--confirm`; preview is
//! read-only; apply runs under the fixed lock order (hosts → secrets), reuses
//! the phase-0/1 `.commit.json` redo log through the ordinary save path, and
//! writes an `app.log` audit entry. Replays are idempotent.
//!
//! Known limit (K8 in `docs/KNOWN-ISSUES.md`):
//! an *old* binary that unlocks the store runs `migrate_plaintext_secrets()` at
//! startup and re-seals tagged references as plaintext, stripping the tags
//! again. Retag does not protect against a later downgrade; downgrading still
//! requires reverting references to the bare marker first (configuration
//! guide).

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;

use crate::{diagnostics, secrets, store};

fn audit(applied: bool, fields: Value) {
    let _ = diagnostics::append_diagnostic_log(
        "info",
        "secrets_retag",
        &format!(
            "secrets retag {}",
            if applied { "applied" } else { "previewed" }
        ),
        Some(fields),
    );
}

/// One bare `$agent2ssh-secret$` reference found in `hosts.json`.
#[derive(Debug, Clone, Serialize)]
pub struct BareRef {
    /// `host` or `proxy`
    pub kind: String,
    /// Host name or proxy id.
    pub name: String,
    /// `password` or `passphrase`.
    pub field: String,
    /// Credential account the reference resolves through.
    pub account: String,
    /// True when `secrets.enc` has an entry for the account, so the reference
    /// can be upgraded once the entry carries a tag.
    pub resolvable: bool,
}

/// Read-only result of scanning the config pair for legacy shapes.
#[derive(Debug, Clone, Serialize, Default)]
pub struct RetagScan {
    /// Accounts in `secrets.enc` whose entry has no valid content tag.
    pub legacy_entries: Vec<String>,
    /// Bare references in `hosts.json`.
    pub bare_refs: Vec<BareRef>,
    /// How many bare references point at an account that has a store entry
    /// (these are the ones retag can upgrade).
    pub upgradable_refs: usize,
}

/// Scan `hosts.json` and `secrets.enc` **without decrypting or locking**.
///
/// Bare references that point at a missing store entry are reported (with
/// `resolvable: false`) and kept intact; a tag cannot be assigned to a
/// nonexistent ciphertext.
pub fn scan() -> Result<RetagScan> {
    let mut result = RetagScan {
        legacy_entries: secrets::legacy_tag_accounts(),
        ..Default::default()
    };

    let dir = store::config_dir()?;
    let raw =
        fs::read_to_string(dir.join("hosts.json")).context("cannot read hosts config for retag")?;
    // Match the configuration loader's schema validation before presenting a
    // preview; malformed data is not evidence that no references exist.
    let _: crate::types::AppConfig =
        serde_json::from_str(&raw).context("cannot parse hosts config for retag")?;
    let config: Value =
        serde_json::from_str(&raw).context("cannot parse hosts config for retag")?;

    let mut push_ref = |kind: &str, name: &str, field: &str, value: &str| {
        if crate::secrets::parse_secret_ref(value) == Some(crate::secrets::SecretRef::Bare) {
            let account = match (kind, field) {
                ("host", "passphrase") => secrets::host_passphrase_account(name),
                ("host", _) => secrets::host_account(name),
                _ => secrets::proxy_account(name),
            };
            result.bare_refs.push(BareRef {
                kind: kind.to_string(),
                name: name.to_string(),
                field: field.to_string(),
                account: account.clone(),
                resolvable: secrets::secret_exists(&account),
            });
        }
    };

    if let Some(hosts) = config.get("hosts").and_then(Value::as_array) {
        for host in hosts {
            let Some(name) = host.get("name").and_then(Value::as_str) else {
                continue;
            };
            for field in ["password", "passphrase"] {
                if let Some(value) = host.get(field).and_then(Value::as_str) {
                    push_ref("host", name, field, value);
                }
            }
        }
    }
    if let Some(proxies) = config.get("proxies").and_then(Value::as_array) {
        for proxy in proxies {
            let Some(id) = proxy.get("id").and_then(Value::as_str) else {
                continue;
            };
            if let Some(value) = proxy.get("password").and_then(Value::as_str) {
                push_ref("proxy", id, "password", value);
            }
        }
    }

    result.upgradable_refs = result
        .bare_refs
        .iter()
        .filter(|reference| reference.resolvable)
        .count();
    Ok(result)
}

/// Preview for the CLI: scan plus an `app.log` audit entry. Touches no config
/// file.
pub fn preview() -> Result<RetagScan> {
    if !secrets::is_initialized() {
        bail!("no credential store exists; nothing to retag");
    }
    secrets::validate_retag_store()?;
    let scan = scan()?;
    audit(
        false,
        json!({
            "legacy_entries": scan.legacy_entries,
            "bare_refs": scan.bare_refs.len(),
            "upgradable_refs": scan.upgradable_refs,
        }),
    );
    Ok(scan)
}

/// Human-readable rendering of a preview, in the `config-repair` style.
pub fn describe(scan: &RetagScan) -> String {
    let mut out = String::from("secrets retag — preview\n");
    out.push_str(&format!(
        "  credential entries without a content tag: {}\n",
        scan.legacy_entries.len()
    ));
    for account in &scan.legacy_entries {
        out.push_str(&format!("    - {account}\n"));
    }
    out.push_str(&format!(
        "  bare $agent2ssh-secret$ references in hosts.json: {} ({} upgradable)\n",
        scan.bare_refs.len(),
        scan.upgradable_refs
    ));
    for reference in &scan.bare_refs {
        out.push_str(&format!(
            "    - {} '{}' {} -> {}{}\n",
            reference.kind,
            reference.name,
            reference.field,
            reference.account,
            if reference.resolvable {
                String::new()
            } else {
                " (no store entry — left as-is)".to_string()
            }
        ));
    }
    if scan.legacy_entries.is_empty() && scan.bare_refs.is_empty() {
        out.push_str(
            "  Every entry and reference is tagged; --confirm only aligns generations if needed.\n",
        );
    } else if scan.legacy_entries.is_empty() && scan.upgradable_refs == 0 {
        out.push_str(
            "  No references can be upgraded until their missing credentials are restored.\n",
        );
        out.push_str("  --confirm keeps them unchanged and only aligns generations if needed.\n");
    } else {
        out.push_str(
            "  With --confirm: legacy entries get a fresh content tag (plaintext unchanged),\n",
        );
        out.push_str("  resolvable bare references are upgraded to $agent2ssh-secret$:<tag>,\n");
        out.push_str("  unresolved references remain unchanged, and hosts.json\n");
        out.push_str("  secrets_generation is re-aligned with secrets.enc.generation.\n");
        out.push_str(
            "  NOTE: an OLD agent2ssh binary that unlocks the store downgrades this again\n",
        );
        out.push_str(
            "  (it re-seals tagged references as plaintext at startup). Downgrade first\n",
        );
        out.push_str(
            "  requires reverting references to the bare marker — see the configuration guide.\n",
        );
    }
    out
}

/// Apply the retag. Returns a human-readable summary.
///
/// Fails closed on a locked store **before any lock is taken or any file is
/// touched**, so a refused run leaves the config directory byte-for-byte
/// unchanged. The write half reuses existing machinery only: the entry
/// rewrite goes through `secrets::retag_legacy_entries()` (one atomic replace
/// under `.secrets.lock`), and the `hosts.json` half goes through the ordinary
/// `save_config_unlocked` path, which provides the `.commit.json` redo log,
/// the generation re-stamp and the paired snapshot. The lock order is the
/// fixed one: hosts (via `store_write_lock`) before secrets.
pub fn apply() -> Result<String> {
    if !secrets::is_initialized() {
        bail!("no credential store exists; nothing to retag");
    }
    // Fail closed before touching anything: no lock file, no audit entry, no
    // write of any kind while the store is locked.
    if !secrets::is_unlocked() && !secrets::try_unlock_from_env() {
        bail!(
            "credential store is locked — nothing was read or written; unlock it (desktop) or set AGENT2SSH_MASTER_PASSWORD and re-run"
        );
    }

    // Hosts lock first, then secrets — the fixed order. `store_write_lock`
    // also replays any commit a crashed writer left behind.
    let _guard = store::store_write_lock()?;
    let before = scan()?;
    // Validate the current hosts file before rewriting secrets, but defer
    // credential resolution until after the retag is complete.
    let mut config = store::load_config_for_retag()?;
    secrets::validate_retag_store()?;
    let store_generation =
        secrets::store_generation().context("cannot read credential store generation for retag")?;
    if config.secrets_generation > store_generation {
        bail!(
            "credential store is behind hosts.json (config generation {}, store generation {}); refusing to retag until secrets.enc is restored",
            config.secrets_generation,
            store_generation
        );
    }
    if before.legacy_entries.is_empty()
        && before.upgradable_refs == 0
        && store_generation == config.secrets_generation
    {
        audit(
            false,
            json!({ "nothing_to_do": true, "unresolved_refs": before.bare_refs.len() }),
        );
        return Ok(if before.bare_refs.is_empty() {
            "Nothing to retag: every credential entry has a content tag and every reference is tagged.".to_string()
        } else {
            format!("Nothing to retag: {} unresolved bare reference(s) remain unchanged; restore their credentials before upgrading them.", before.bare_refs.len())
        });
    }
    let retagged = secrets::retag_legacy_entries()?;
    store::resolve_config_for_retag(&mut config);

    // Re-seal `hosts.json` through the ordinary save path: resolvable bare
    // references become plaintext; unresolved references remain markers.
    // `save_map` keeps the tag of an unchanged secret, which
    // is now the fresh tag written above, so references come out tagged and
    // `secrets_generation` is stamped to the new generation. This is where the
    // `.commit.json` redo log is written if secrets changed.
    store::save_config_unlocked(&config)?;

    let after = scan()?;
    let hosts_generation = store::config_dir()
        .ok()
        .and_then(|dir| fs::read_to_string(dir.join("hosts.json")).ok())
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|config| config.get("secrets_generation").and_then(Value::as_u64));
    let store_generation = secrets::store_generation();
    if hosts_generation.is_none() || hosts_generation != store_generation {
        bail!(
            "generation mismatch after retag (hosts.json {:?}, secrets.enc {:?}); inspect the config directory",
            hosts_generation,
            store_generation
        );
    }
    if !after.legacy_entries.is_empty() {
        bail!(
            "{} legacy entry/entries remain after retag; inspect the config directory",
            after.legacy_entries.len()
        );
    }

    audit(
        true,
        json!({
            "legacy_entries_before": before.legacy_entries,
            "entries_retagged": retagged,
            "bare_refs_before": before.bare_refs.len(),
            "bare_refs_remaining": after.bare_refs.len(),
            "generation": store_generation,
        }),
    );
    Ok(format!(
        "Retagged {retagged} credential entries; hosts.json now carries {} tagged reference(s), {} left bare (no store entry); generation aligned at {}.",
        before.upgradable_refs,
        after.bare_refs.len(),
        store_generation.unwrap_or(0),
    ))
}
