//! Import a Netcatty vault backup into this app's configuration.
//!
//! ## What the input actually is
//!
//! Netcatty is an Electron app. Its "vault backup" file is an envelope whose
//! `payloadData` holds the real `SyncPayload` JSON in one of two encodings:
//!
//! - `plain-json-v1` — the JSON verbatim.
//! - `safeStorage-v1` — `safeStorage.encryptString(raw)` from Electron's main
//!   process, base64'd. That is Chromium's OSCrypt, so the ciphertext starts
//!   with a version tag (`v10` on macOS/Windows, `v11` on Linux) and is
//!   encrypted with a key the OS credential store holds, not with anything in
//!   the file.
//!
//! A bare `SyncPayload` JSON is accepted too, which is what a user gets after
//! decrypting the envelope elsewhere.
//!
//! ## Why OSCrypt is reimplemented here
//!
//! There is no library to call: the container is Electron's, and the key lives
//! in Keychain/DPAPI/libsecret. Each platform's scheme is fixed and public, so
//! the cost is one small decryptor per OS rather than a dependency on Electron:
//!
//! - **macOS** — Keychain item `"<app> Safe Storage"` holds a password; the key
//!   is PBKDF2-HMAC-SHA1(password, `"saltysalt"`, 1003, 16) and the payload
//!   after the `v10` tag is AES-128-CBC with sixteen 0x20 bytes as the IV.
//! - **Windows** — the payload after `v10` is DPAPI-protected directly (no
//!   second cipher layer), so `CryptUnprotectData` returns the JSON.
//! - **Linux** — the password comes from libsecret via `secret-tool`, the same
//!   KDF as macOS, and the tag is `v11`.
//!
//! Only the macOS path has been verified against a real backup on this
//! machine. The Windows and Linux paths are written from the documented scheme
//! and compile for their targets, but running them needs a host of that OS;
//! when the key cannot be read the error says so instead of guessing, and
//! `plain-json-v1` remains the portable fallback.
//!
//! ## What the import keeps, and what it drops
//!
//! Kept: name/host/user/port, password, group, tags, jump host (Netcatty host
//! chains are single-hop here; a longer chain is reported rather than
//! truncated), private keys, startup command, and command snippets.
//!
//! Dropped, and reported: terminal appearance (theme/font/size/charset),
//! `sftpSudo`, vault notes, and port-forwarding rules — this app has no
//! persisted rule store for forwards (`app_state.forwards` is a runtime map),
//! so those are listed with the arguments needed to recreate them instead of
//! being silently discarded.

use crate::store::{self, store_write_lock};
use crate::types::{HostGroup, HostProfile};
use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Default Keychain/libsecret service name. Electron derives it from
/// `app.getName()`, which for this app is the lowercase package name.
const DEFAULT_SAFE_STORAGE_SERVICE: &str = "netcatty Safe Storage";

// ── Envelope and payload ─────────────────────────────────────────────────────

/// One host as Netcatty stores it. Only the fields this importer maps are
/// named; everything else in the record is ignored on purpose, so a Netcatty
/// upgrade that adds fields does not break the import.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetcattyHost {
    pub id: String,
    pub label: String,
    pub hostname: String,
    pub port: Option<u16>,
    pub username: String,
    pub password: Option<String>,
    pub group: Option<String>,
    pub tags: Vec<String>,
    pub identity_file_id: Option<String>,
    pub identity_file_paths: Vec<String>,
    pub host_chain: Option<NetcattyHostChain>,
    pub startup_command: Option<String>,
    pub protocol: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetcattyHostChain {
    pub host_ids: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetcattyKey {
    pub id: String,
    pub label: String,
    pub private_key: String,
    pub public_key: Option<String>,
    pub passphrase: Option<String>,
    pub file_path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetcattySnippet {
    pub label: String,
    pub command: String,
    pub description: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetcattyForwardRule {
    pub label: Option<String>,
    /// Netcatty names this field `type`, so it needs an explicit rename.
    #[serde(rename = "type")]
    pub rule_type: Option<String>,
    pub local_port: Option<u16>,
    pub bind_address: Option<String>,
    pub remote_host: Option<String>,
    pub remote_port: Option<u16>,
    pub host_id: Option<String>,
}

/// The subset of Netcatty's `SyncPayload` this importer consumes.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncPayload {
    pub hosts: Vec<NetcattyHost>,
    pub keys: Vec<NetcattyKey>,
    pub snippets: Vec<NetcattySnippet>,
    pub custom_groups: Vec<String>,
    pub port_forwarding_rules: Vec<NetcattyForwardRule>,
}

// ── Options and report ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Override the credential-store service name. Needed when the backup was
    /// written by a differently-named build of the app.
    pub service: Option<String>,
    /// Decode and map everything, write nothing.
    pub dry_run: bool,
    /// Map hosts but leave `password` and `key_path` empty. Keys referenced by
    /// the backup are then not written either.
    pub skip_credentials: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub dry_run: bool,
    pub hosts_planned: usize,
    pub hosts_imported: Vec<String>,
    pub hosts_skipped_existing: Vec<String>,
    /// Imported under a suffixed name because the backup repeated a label.
    pub hosts_renamed: Vec<(String, String)>,
    pub keys_written: Vec<String>,
    pub keys_skipped_existing: Vec<String>,
    /// Passwords verified as sealed (`SECRET_REF`) in `hosts.json` after the
    /// write. Zero when the backup carried none.
    pub passwords_sealed: usize,
    /// Group ids that had to be registered so the hosts would not be folded
    /// into `default` by the save-time normalizer.
    pub groups_created: Vec<String>,
    pub snippets_imported: Vec<String>,
    pub snippets_skipped_existing: Vec<String>,
    pub jump_hosts_linked: usize,
    /// Non-fatal problems: dropped fields, unreachable key material, chains
    /// that cannot be represented.
    pub warnings: Vec<String>,
    /// Forwarding rules, rendered as the command that recreates each one.
    pub forwards_not_imported: Vec<String>,
}

// ── Entry point ──────────────────────────────────────────────────────────────

/// Read `path`, decode it, and merge it into the configuration.
///
/// Existing hosts win: a host whose name is already taken is skipped, the same
/// contract `import_ssh_config_core` follows. Names repeated *within* the
/// backup are suffixed (`name-2`) rather than dropped, because Netcatty keys
/// hosts by id and allows the same label in two groups.
pub fn import_netcatty_core(path: &Path, options: &ImportOptions) -> Result<ImportReport> {
    let payload = read_backup(path, options)?;

    // Plan first, write second: a dry run must not touch the key directory.
    let mut report = ImportReport {
        dry_run: options.dry_run,
        hosts_planned: payload.hosts.len(),
        ..ImportReport::default()
    };

    let key_plan = plan_keys(&payload, options, &mut report)?;

    let existing_config = store::load_config()?;
    let existing_names: HashSet<String> = existing_config
        .hosts
        .iter()
        .map(|h| h.name.clone())
        .collect();
    let existing_keys: HashSet<String> = crate::keys::list_keys_core()?
        .into_iter()
        .map(|k| k.name)
        .collect();

    let (hosts, id_to_name) =
        plan_hosts(&payload, &key_plan, &existing_names, options, &mut report);
    let groups = plan_groups(&payload, &hosts, &existing_config.groups);
    report.groups_created = groups.iter().map(|g| g.id.clone()).collect();
    let snippet_plan = plan_snippets(&payload, &mut report);

    collect_dropped(&payload, &id_to_name, &mut report);

    if options.dry_run {
        report.hosts_imported = hosts.iter().map(|h| h.name.clone()).collect();
        report.keys_written = key_plan
            .iter()
            .filter(|k| !existing_keys.contains(&k.name))
            .map(|k| k.name.clone())
            .collect();
        report.snippets_imported = snippet_plan
            .iter()
            .filter(|s| !existing_snippet_names().contains(&s.name))
            .map(|s| s.name.clone())
            .collect();
        return Ok(report);
    }

    // A locked credential store makes `save_config` fall back to writing the
    // password verbatim, which is exactly what a migration must not do: the
    // plaintext would sit in `hosts.json` until someone unlocks the store and
    // it gets rewritten. Refuse instead, and name the way forward.
    //
    // The check mirrors `store::save_config`: it accepts a store unlocked in
    // this process *or* one that can be unlocked from the environment, because
    // a one-shot CLI invocation has no desktop session to unlock it first.
    let with_password = hosts.iter().filter(|h| h.password.is_some()).count();
    if with_password > 0 && !crate::secrets::is_unlocked() && !crate::secrets::try_unlock_from_env()
    {
        let guidance = if crate::secrets::is_initialized() {
            "the credential store exists but is locked — export AGENT2SSH_MASTER_PASSWORD \
             for this command"
        } else {
            "no credential store exists yet — create one with \
             `agent2ssh secrets set-password`, then export AGENT2SSH_MASTER_PASSWORD"
        };
        return Err(anyhow!(
            "{with_password} host(s) in this backup carry passwords, but {guidance}. \
             Importing now would write them into hosts.json in plaintext. \
             Alternatively re-run with --skip-credentials to import everything except \
             the passwords."
        ));
    }

    // Keys are written before the config so a host never points at a file that
    // does not exist yet. A failure here leaves orphan key files at worst —
    // harmless, and re-running the import skips names that already exist.
    for key in &key_plan {
        if existing_keys.contains(&key.name) {
            report.keys_skipped_existing.push(key.name.clone());
            continue;
        }
        write_key(key)?;
        report.keys_written.push(key.name.clone());
    }

    let imported_names: Vec<String> = hosts.iter().map(|h| h.name.clone()).collect();
    {
        let _guard = store_write_lock()?;
        let mut config = store::load_config()?;
        // Groups first: the normalizer runs over hosts *and* groups together.
        for group in &groups {
            if !config.groups.iter().any(|existing| existing.id == group.id) {
                config.groups.push(group.clone());
            }
        }
        for host in &hosts {
            config.hosts.push(host.clone());
        }
        config.hosts.sort_by(|a, b| a.name.cmp(&b.name));
        store::save_config_unlocked(&config)?;
    }
    report.hosts_imported = imported_names;

    // Verify against the file, not against `load_config`: that function resolves
    // `SECRET_REF` back to plaintext on load, so it cannot tell a sealed value
    // from a leaked one. On disk a sealed password must be the marker.
    if with_password > 0 {
        let path = store::config_dir()?.join("hosts.json");
        let on_disk = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to re-read {}", path.display()))?;
        let disk_config: crate::types::AppConfig = serde_json::from_str(&on_disk)
            .with_context(|| format!("{} is not valid JSON after saving", path.display()))?;
        let leaked = disk_config
            .hosts
            .iter()
            .filter(|h| {
                h.password
                    .as_deref()
                    .is_some_and(|value| value != crate::secrets::SECRET_REF)
            })
            .count();
        if leaked > 0 {
            return Err(anyhow!(
                "{leaked} password(s) landed in hosts.json in plaintext — the credential store \
                 accepted the write but did not seal them. Unlock the store, then re-save the \
                 configuration so the values are migrated."
            ));
        }
        report.passwords_sealed = with_password;
    }

    merge_snippets(&snippet_plan, &mut report)?;

    Ok(report)
}

// ── Decoding ─────────────────────────────────────────────────────────────────

fn read_backup(path: &Path, options: &ImportOptions) -> Result<SyncPayload> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("{} is not valid JSON", path.display()))?;

    // A bare SyncPayload has no payloadData; an envelope always does.
    let Some(data) = value.get("payloadData").and_then(|v| v.as_str()) else {
        return serde_json::from_value(value).with_context(|| {
            format!("{} is not a Netcatty backup or SyncPayload", path.display())
        });
    };

    let encoding = value
        .get("payloadEncoding")
        .and_then(|v| v.as_str())
        .unwrap_or("plain-json-v1");

    let json = match encoding {
        "plain-json-v1" => data.to_string(),
        "safeStorage-v1" => {
            let ciphertext = base64::engine::general_purpose::STANDARD
                .decode(data)
                .context("payloadData is not valid base64")?;
            let service = options
                .service
                .as_deref()
                .unwrap_or(DEFAULT_SAFE_STORAGE_SERVICE);
            let plain = decode_safe_storage(&ciphertext, service)?;
            String::from_utf8(plain).context("the decrypted payload is not UTF-8")?
        }
        other => {
            return Err(anyhow!(
                "unsupported payloadEncoding '{other}' (expected plain-json-v1 or safeStorage-v1)"
            ))
        }
    };

    serde_json::from_str(&json).context("the decoded payload is not a Netcatty SyncPayload")
}

/// Decrypt Chromium OSCrypt output. Each platform has its own key source but
/// the same call shape, so the tag check lives here and the cipher work is
/// delegated.
fn decode_safe_storage(payload: &[u8], service: &str) -> Result<Vec<u8>> {
    #[cfg(target_os = "macos")]
    {
        let password = keychain_password(service)?;
        let tag = payload
            .get(..3)
            .ok_or_else(|| anyhow!("the encrypted payload is too short to carry a version tag"))?;
        if tag != b"v10" {
            return Err(anyhow!(
                "unsupported OSCrypt tag {:?} (expected v10)",
                String::from_utf8_lossy(tag)
            ));
        }
        return aes_128_cbc_decrypt(&password, &payload[3..]);
    }

    #[cfg(target_os = "windows")]
    {
        // Electron on Windows hands the string to DPAPI itself; there is no
        // second AES layer, so the tag is stripped and the blob unsealed.
        let _ = service;
        let blob = payload.strip_prefix(b"v10").unwrap_or(payload);
        return dpapi_unprotect(blob);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let password = libsecret_password(service)?;
        let tag = payload
            .get(..3)
            .ok_or_else(|| anyhow!("the encrypted payload is too short to carry a version tag"))?;
        if tag != b"v11" && tag != b"v10" {
            return Err(anyhow!(
                "unsupported OSCrypt tag {:?} (expected v10 or v11)",
                String::from_utf8_lossy(tag)
            ));
        }
        return aes_128_cbc_decrypt(&password, &payload[3..]);
    }

    #[allow(unreachable_code)]
    Err(anyhow!(
        "safeStorage decryption is not implemented for this platform; \
         export a plain JSON backup from Netcatty instead"
    ))
}

// ── Cipher primitives ────────────────────────────────────────────────────────

/// PBKDF2-HMAC-SHA1, the derivation Chromium fixes for its OSCrypt key. ~20
/// lines over `hmac` beats adding a KDF crate whose digest major would have to
/// agree with ours.
fn pbkdf2_hmac_sha1(password: &[u8], salt: &[u8], iterations: u32, output: &mut [u8]) {
    use hmac::{Hmac, Mac};
    use sha1::Sha1;
    type HmacSha1 = Hmac<Sha1>;

    let iterations = iterations.max(1);
    let mut block: u32 = 1;
    let mut written = 0;
    while written < output.len() {
        let mut mac = HmacSha1::new_from_slice(password).expect("HMAC accepts any key length");
        mac.update(salt);
        mac.update(&block.to_be_bytes());
        let mut u = mac.finalize().into_bytes().to_vec();
        let mut t = u.clone();
        for _ in 1..iterations {
            let mut mac = HmacSha1::new_from_slice(password).expect("HMAC accepts any key length");
            mac.update(&u);
            u = mac.finalize().into_bytes().to_vec();
            for (a, b) in t.iter_mut().zip(u.iter()) {
                *a ^= *b;
            }
        }
        let take = (output.len() - written).min(t.len());
        output[written..written + take].copy_from_slice(&t[..take]);
        written += take;
        block += 1;
    }
}

/// AES-128-CBC with the OSCrypt parameters: key from PBKDF2 over the store
/// password, a fixed sixteen-space IV, PKCS#7 padding. macOS and Linux both
/// use this; Windows hands the payload straight to DPAPI instead.
#[cfg(unix)]
fn aes_128_cbc_decrypt(password: &str, ciphertext: &[u8]) -> Result<Vec<u8>> {
    use aes::cipher::{BlockDecrypt, BlockSizeUser, KeyInit};
    use aes::Aes128;

    const SALT: &[u8] = b"saltysalt";
    const ITERATIONS: u32 = 1003;

    let mut key = [0u8; 16];
    pbkdf2_hmac_sha1(password.as_bytes(), SALT, ITERATIONS, &mut key);
    let iv = [0x20u8; 16];

    let block_size = <Aes128 as BlockSizeUser>::block_size();
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(block_size) {
        return Err(anyhow!(
            "the encrypted body is not a whole number of AES blocks"
        ));
    }
    let cipher = Aes128::new_from_slice(&key).map_err(|_| anyhow!("invalid AES key length"))?;

    let mut previous = iv.to_vec();
    let mut plain = Vec::with_capacity(ciphertext.len());
    for chunk in ciphertext.chunks_exact(block_size) {
        let mut block = chunk.to_vec();
        cipher.decrypt_block(aes::cipher::generic_array::GenericArray::from_mut_slice(
            &mut block,
        ));
        for (index, byte) in block.iter().enumerate() {
            plain.push(byte ^ previous[index]);
        }
        previous = chunk.to_vec();
    }

    let pad = *plain
        .last()
        .ok_or_else(|| anyhow!("the decrypted payload is empty"))? as usize;
    if pad == 0
        || pad > block_size
        || pad > plain.len()
        || plain[plain.len() - pad..]
            .iter()
            .any(|b| *b as usize != pad)
    {
        return Err(anyhow!(
            "the store password did not decrypt this payload (padding is invalid) — \
             the backup may come from a different app name or another machine"
        ));
    }
    plain.truncate(plain.len() - pad);
    Ok(plain)
}

// ── Platform key sources ─────────────────────────────────────────────────────

/// How long to wait for the OS credential store before giving up.
///
/// Reading the key can raise an interactive prompt (macOS shows `SecurityAgent`
/// the first time a given binary asks, or after "Allow once" expires). Without a
/// deadline a scripted import would hang forever with nothing on stdout to
/// explain why, so the wait is bounded and the timeout says what to approve.
#[cfg(unix)]
const KEY_LOOKUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Run `program` with `args`, returning its stdout, and give up after
/// [`KEY_LOOKUP_TIMEOUT`].
#[cfg(unix)]
fn run_with_timeout(program: &str, args: &[&str], hint: &str) -> Result<Vec<u8>> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run `{program}`"))?;

    let deadline = Instant::now() + KEY_LOOKUP_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(anyhow!(
                    "`{program}` did not answer within {}s. {hint}",
                    KEY_LOOKUP_TIMEOUT.as_secs()
                ));
            }
            Err(error) => {
                return Err(error).with_context(|| format!("failed to wait for `{program}`"))
            }
        }
    }

    // The child has exited, so draining the pipes cannot block on it.
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_end(&mut stdout).ok();
    }
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_end(&mut stderr).ok();
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(anyhow!(
            "`{program}` failed ({}): {}",
            status,
            String::from_utf8_lossy(&stderr).trim()
        ));
    }
    Ok(stdout)
}

#[cfg(target_os = "macos")]
fn keychain_password(service: &str) -> Result<String> {
    let stdout = match run_with_timeout(
        "security",
        &["find-generic-password", "-s", service, "-w"],
        "macOS is most likely showing a Keychain access prompt for this binary — approve it \
         (choosing \"Always Allow\" makes later runs silent), then re-run the import. \
         Running `security find-generic-password -s \"<service>\" -w` once in a terminal \
         does the same thing.",
    ) {
        Ok(stdout) => stdout,
        Err(error) => {
            return Err(error.context(format!(
                "no readable Keychain entry named '{service}' — the backup may have been \
                 encrypted on another machine, or by a build with a different app name. \
                 Pass --service <name> to override, or export a plain JSON backup from Netcatty."
            )))
        }
    };
    Ok(String::from_utf8(stdout)
        .context("the Keychain password is not UTF-8")?
        .trim()
        .to_string())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn libsecret_password(service: &str) -> Result<String> {
    let stdout = match run_with_timeout(
        "secret-tool",
        &["lookup", "application", service],
        "the desktop keyring may be prompting for unlock; unlock it and re-run the import.",
    ) {
        Ok(stdout) => stdout,
        Err(error) => {
            return Err(error.context(format!(
                "libsecret has no readable entry for '{service}'. Pass --service <name> to \
                 override, or export a plain JSON backup from Netcatty."
            )))
        }
    };
    if stdout.is_empty() {
        return Err(anyhow!(
            "libsecret returned an empty password for '{service}'"
        ));
    }
    Ok(String::from_utf8(stdout)
        .context("the libsecret password is not UTF-8")?
        .trim()
        .to_string())
}

#[cfg(target_os = "windows")]
fn dpapi_unprotect(blob: &[u8]) -> Result<Vec<u8>> {
    // `LocalFree` lives under System::Memory in windows-sys 0.48, not under
    // Foundation where the allocation-shaped name suggests it should be.
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};
    use windows_sys::Win32::System::Memory::LocalFree;

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: blob.len() as u32,
        pbData: blob.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: both blobs point at live buffers for the duration of the call;
    // the output buffer is allocated by the API and released with LocalFree.
    let ok = unsafe {
        CryptUnprotectData(
            &input as *const _,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            &mut output as *mut _,
        )
    };
    if ok == 0 {
        return Err(anyhow!(
            "DPAPI could not unseal the payload — it was encrypted under another Windows \
             user or machine. Export a plain JSON backup from Netcatty instead."
        ));
    }
    // SAFETY: on success the API set pbData/cbData to a LocalAlloc'd buffer.
    let plain = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let owned = plain.to_vec();
    unsafe {
        LocalFree(output.pbData as isize);
    }
    Ok(owned)
}

// ── Planning ─────────────────────────────────────────────────────────────────

struct PlannedKey {
    /// Netcatty's key id, kept so a host's `identityFileId` resolves to the
    /// right plan even when an earlier key was skipped.
    id: String,
    name: String,
    private_key: String,
    public_key: Option<String>,
    /// Set when the backup referenced an on-disk key instead of embedding it.
    source_path: Option<PathBuf>,
}

fn plan_keys(
    payload: &SyncPayload,
    options: &ImportOptions,
    report: &mut ImportReport,
) -> Result<Vec<PlannedKey>> {
    if options.skip_credentials {
        return Ok(Vec::new());
    }
    let mut used: HashSet<String> = HashSet::new();
    let mut planned = Vec::new();
    for key in &payload.keys {
        // A filePath reference means the private half lives outside the backup;
        // pointing at it is better than writing a copy.
        let embedded = !key.private_key.trim().is_empty();
        if !embedded && key.file_path.is_none() {
            report.warnings.push(format!(
                "key '{}' carries neither private key material nor a file path; skipped",
                key.label
            ));
            continue;
        }
        let name = unique_name(&sanitize_name(&key.label, "netcatty-key"), &used);
        used.insert(name.clone());
        planned.push(PlannedKey {
            id: key.id.clone(),
            name,
            private_key: key.private_key.clone(),
            public_key: key.public_key.clone(),
            source_path: if embedded {
                None
            } else {
                key.file_path.as_ref().map(PathBuf::from)
            },
        });
    }
    Ok(planned)
}

fn write_key(key: &PlannedKey) -> Result<()> {
    let dir = store::config_dir()?.join("keys");
    std::fs::create_dir_all(&dir)?;
    let private_path = dir.join(&key.name);
    store::write_private_text(&private_path, &key.private_key)
        .with_context(|| format!("failed to write {}", private_path.display()))?;
    if let Some(public) = &key.public_key {
        let public_path = dir.join(format!("{}.pub", key.name));
        std::fs::write(&public_path, format!("{}\n", public.trim()))
            .with_context(|| format!("failed to write {}", public_path.display()))?;
    }
    Ok(())
}

/// Build the host profiles and the id→final-name map a jump host resolves
/// through.
fn plan_hosts(
    payload: &SyncPayload,
    keys: &[PlannedKey],
    existing: &HashSet<String>,
    options: &ImportOptions,
    report: &mut ImportReport,
) -> (Vec<HostProfile>, HashMap<String, String>) {
    let key_by_id: HashMap<&str, &PlannedKey> = keys
        .iter()
        .map(|planned| (planned.id.as_str(), planned))
        .collect();
    let key_dir = store::config_dir().map(|d| d.join("keys")).ok();

    // Pass 1: names, so pass 2 can point `jump_host` at a final name.
    let mut id_to_name: HashMap<String, String> = HashMap::new();
    let mut taken: HashSet<String> = existing.clone();
    for host in &payload.hosts {
        let base = sanitize_name(&host.label, "netcatty-host");
        if taken.contains(&base) && !existing.contains(&base) {
            let renamed = unique_name(&base, &taken);
            report
                .hosts_renamed
                .push((host.label.clone(), renamed.clone()));
            taken.insert(renamed.clone());
            id_to_name.insert(host.id.clone(), renamed);
        } else {
            taken.insert(base.clone());
            id_to_name.insert(host.id.clone(), base);
        }
    }

    let mut profiles = Vec::new();
    for host in &payload.hosts {
        let Some(name) = id_to_name.get(&host.id).cloned() else {
            continue;
        };
        if existing.contains(&name) {
            report.hosts_skipped_existing.push(name);
            continue;
        }

        let mut key_path = None;
        if !options.skip_credentials {
            // An explicit path wins: it is what the user's own ssh setup reads.
            if let Some(path) = host.identity_file_paths.first() {
                key_path = Some(path.clone());
            } else if let Some(id) = &host.identity_file_id {
                match key_by_id.get(id.as_str()) {
                    Some(planned) => {
                        key_path = Some(match &planned.source_path {
                            Some(source) => source.display().to_string(),
                            None => key_dir
                                .as_ref()
                                .map(|d| d.join(&planned.name).display().to_string())
                                .unwrap_or_else(|| planned.name.clone()),
                        });
                    }
                    None => report.warnings.push(format!(
                        "host '{}' references key id '{id}', which the backup does not contain",
                        host.label
                    )),
                }
            }
        }

        let jump_host = host
            .host_chain
            .as_ref()
            .and_then(|chain| chain.host_ids.first())
            .and_then(|id| id_to_name.get(id).cloned());
        if jump_host.is_some() {
            report.jump_hosts_linked += 1;
        }
        if let Some(chain) = &host.host_chain {
            if chain.host_ids.len() > 1 {
                report.warnings.push(format!(
                    "host '{}' has a {}-hop chain; only the first hop was kept \
                     (this app's jump_host is a single alias)",
                    host.label,
                    chain.host_ids.len()
                ));
            }
        }

        profiles.push(HostProfile {
            ai_allowed: true,
            name,
            host: host.hostname.clone(),
            user: non_empty(&host.username),
            port: host.port,
            key_path,
            password: if options.skip_credentials {
                None
            } else {
                host.password.clone().filter(|p| !p.is_empty())
            },
            jump_host,
            proxy_id: None,
            risk_override: None,
            tags: host.tags.clone(),
            group: host
                .group
                .clone()
                .filter(|g| !g.trim().is_empty())
                .unwrap_or_else(crate::types::default_host_group),
            env: None,
            role: None,
            owner: None,
            init_command: host
                .startup_command
                .clone()
                .filter(|c| !c.trim().is_empty()),
            passphrase: None,
        });
    }
    (profiles, id_to_name)
}

/// Register the groups the imported hosts refer to.
///
/// `normalize_config` resets any host whose `group` is not a registered
/// `HostGroup` id, so writing hosts without their groups would silently move
/// every one of them into `default` — `hosts.json` would look imported while
/// the organization the backup encoded was gone.
fn plan_groups(
    payload: &SyncPayload,
    hosts: &[HostProfile],
    existing: &[HostGroup],
) -> Vec<HostGroup> {
    let default_group = crate::types::default_host_group();
    let mut wanted: Vec<String> = payload.custom_groups.clone();
    wanted.extend(hosts.iter().map(|h| h.group.clone()));
    wanted.retain(|group| {
        let trimmed = group.trim();
        !trimmed.is_empty() && trimmed != default_group
    });
    wanted.sort();
    wanted.dedup();

    let taken: HashSet<String> = existing.iter().map(|g| g.id.clone()).collect();
    let mut order = existing.iter().map(|g| g.sort_order).max().unwrap_or(0);
    let mut planned = Vec::new();
    for id in wanted {
        if taken.contains(&id) {
            continue;
        }
        order += 1;
        planned.push(HostGroup {
            id: id.clone(),
            name: id,
            color: crate::types::default_group_color(),
            sort_order: order,
        });
    }
    planned
}

fn plan_snippets(
    payload: &SyncPayload,
    report: &mut ImportReport,
) -> Vec<crate::snippets::Snippet> {
    let mut used: HashSet<String> = HashSet::new();
    let mut planned = Vec::new();
    for snippet in &payload.snippets {
        if snippet.command.trim().is_empty() {
            report.warnings.push(format!(
                "snippet '{}' has an empty command; skipped",
                snippet.label
            ));
            continue;
        }
        let name = unique_name(&sanitize_name(&snippet.label, "netcatty-snippet"), &used);
        used.insert(name.clone());
        planned.push(crate::snippets::Snippet {
            name,
            command: snippet.command.clone(),
            description: snippet.description.clone().filter(|d| !d.trim().is_empty()),
        });
    }
    planned
}

fn merge_snippets(planned: &[crate::snippets::Snippet], report: &mut ImportReport) -> Result<()> {
    if planned.is_empty() {
        return Ok(());
    }
    let mut existing = crate::snippets::load_snippets()?;
    let mut names: HashSet<String> = existing.iter().map(|s| s.name.clone()).collect();
    for snippet in planned {
        if names.contains(&snippet.name) {
            report.snippets_skipped_existing.push(snippet.name.clone());
            continue;
        }
        names.insert(snippet.name.clone());
        existing.push(snippet.clone());
        report.snippets_imported.push(snippet.name.clone());
    }
    crate::snippets::save_snippets(&existing)?;
    Ok(())
}

fn existing_snippet_names() -> HashSet<String> {
    crate::snippets::load_snippets()
        .map(|list| list.into_iter().map(|s| s.name).collect())
        .unwrap_or_default()
}

fn collect_dropped(
    payload: &SyncPayload,
    id_to_name: &HashMap<String, String>,
    report: &mut ImportReport,
) {
    for rule in &payload.port_forwarding_rules {
        let host = rule
            .host_id
            .as_ref()
            .and_then(|id| id_to_name.get(id))
            .cloned()
            .unwrap_or_else(|| "<unknown host>".to_string());
        let bind = rule.bind_address.as_deref().unwrap_or("127.0.0.1");
        let local = rule
            .local_port
            .map(|p| p.to_string())
            .unwrap_or_else(|| "?".to_string());
        let remote = format!(
            "{}:{}",
            rule.remote_host.as_deref().unwrap_or("?"),
            rule.remote_port
                .map(|p| p.to_string())
                .unwrap_or_else(|| "?".to_string())
        );
        report.forwards_not_imported.push(format!(
            "{}: agent2ssh forward start {host} --local {bind}:{local} --remote {remote}",
            rule.label.as_deref().unwrap_or("forward")
        ));
    }
    if !payload.port_forwarding_rules.is_empty() {
        report.warnings.push(format!(
            "{} port-forwarding rule(s) were not imported: this app keeps forwards in \
             runtime state, not in configuration. Run the printed commands to recreate them.",
            payload.port_forwarding_rules.len()
        ));
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Keep names usable as both a host alias and (for keys) a filename.
fn sanitize_name(raw: &str, fallback: &str) -> String {
    let cleaned: String = raw
        .trim()
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

fn unique_name(base: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_string();
    }
    for suffix in 2..1000 {
        let candidate = format!("{base}-{suffix}");
        if !taken.contains(&candidate) {
            return candidate;
        }
    }
    format!("{base}-{}", uuid::Uuid::new_v4())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6070 vector 1: P="password", S="salt", c=1, dkLen=20.
    #[test]
    fn pbkdf2_matches_rfc6070() {
        let mut out = [0u8; 20];
        pbkdf2_hmac_sha1(b"password", b"salt", 1, &mut out);
        assert_eq!(
            out.to_vec(),
            vec![
                0x0c, 0x60, 0xc8, 0x0f, 0x96, 0x1f, 0x0e, 0x71, 0xf3, 0xa9, 0xb5, 0x24, 0xaf, 0x60,
                0x12, 0x06, 0x2f, 0xe0, 0x37, 0xa6
            ]
        );
    }

    /// RFC 6070 vector 2: c=2.
    #[test]
    fn pbkdf2_matches_rfc6070_with_two_iterations() {
        let mut out = [0u8; 20];
        pbkdf2_hmac_sha1(b"password", b"salt", 2, &mut out);
        assert_eq!(
            out.to_vec(),
            vec![
                0xea, 0x6c, 0x01, 0x4d, 0xc7, 0x2d, 0x6f, 0x8c, 0xcd, 0x1e, 0xd9, 0x2a, 0xce, 0x1d,
                0x41, 0xf0, 0xd8, 0xde, 0x89, 0x57
            ]
        );
    }

    #[test]
    fn sanitize_strips_path_separators_and_falls_back() {
        assert_eq!(sanitize_name("a/b\\c", "fallback"), "a-b-c");
        assert_eq!(sanitize_name("   ", "fallback"), "fallback");
        assert_eq!(sanitize_name("../etc/passwd", "f"), "-etc-passwd");
    }

    #[test]
    fn unique_name_suffixes_within_a_batch() {
        let mut taken = HashSet::new();
        taken.insert("web".to_string());
        assert_eq!(unique_name("web", &taken), "web-2");
        taken.insert("web-2".to_string());
        assert_eq!(unique_name("web", &taken), "web-3");
        assert_eq!(unique_name("db", &taken), "db");
    }

    #[test]
    fn bare_payload_without_envelope_is_accepted() {
        let dir = std::env::temp_dir().join(format!("netcatty-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("payload.json");
        std::fs::write(
            &path,
            r#"{"hosts":[{"id":"1","label":"web","hostname":"example.test","username":"root","port":2222}],"keys":[],"snippets":[]}"#,
        )
        .unwrap();
        let payload = read_backup(&path, &ImportOptions::default()).unwrap();
        assert_eq!(payload.hosts.len(), 1);
        assert_eq!(payload.hosts[0].port, Some(2222));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn plain_json_envelope_is_decoded_without_any_credential_store() {
        let dir = std::env::temp_dir().join(format!("netcatty-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("backup.json");
        let payload = r#"{"hosts":[{"id":"9","label":"db","hostname":"db.test","username":"admin"}],"keys":[],"snippets":[]}"#;
        std::fs::write(
            &path,
            serde_json::json!({
                "formatVersion": 1,
                "payloadEncoding": "plain-json-v1",
                "payloadData": payload,
            })
            .to_string(),
        )
        .unwrap();
        let decoded = read_backup(&path, &ImportOptions::default()).unwrap();
        assert_eq!(decoded.hosts.len(), 1);
        assert_eq!(decoded.hosts[0].label, "db");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_encoding_is_named_in_the_error() {
        let dir = std::env::temp_dir().join(format!("netcatty-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("backup.json");
        std::fs::write(
            &path,
            serde_json::json!({
                "payloadEncoding": "future-v9",
                "payloadData": "AAAA",
            })
            .to_string(),
        )
        .unwrap();
        let err = read_backup(&path, &ImportOptions::default()).unwrap_err();
        assert!(
            err.to_string().contains("future-v9"),
            "the error should name the unsupported encoding: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
