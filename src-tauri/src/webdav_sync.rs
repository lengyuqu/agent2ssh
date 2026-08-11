use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use reqwest::{Client, Method, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use crate::store::{
    config_dir, ensure_config_dir, lock_config_file, restrict_file_to_owner, FileLockGuard,
};

const SYNC_VERSION_FILE: &str = "sync_version.json";
const BACKUP_DIR: &str = "backups";
const REMOTE_FILES_DIR: &str = "files";
const SYNC_MARKER_CLIENT_VERSION: &str = "redacted";

/// Files that are safe and useful to move across machines. This intentionally
/// excludes local SSH trust state, daemon tokens, audit/log data, private SSH
/// keys, and remote daemon token registries.
pub const SYNCABLE_FILES: &[&str] = &[
    "hosts.json",
    "secrets.enc",
    "policy.toml",
    "policy.json",
    "risk_rules.toml",
    "approval_policies.toml",
    "execution_limits.toml",
    "anomaly.toml",
    "playbooks.toml",
];

const LEGACY_UNSYNCABLE_REMOTE_FILES: &[&str] = &["known_hosts.json"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDavSyncFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDavSyncMarker {
    pub schema_version: u32,
    pub global_version: u64,
    pub sync_id: String,
    pub updated_at: DateTime<Utc>,
    pub app_version: String,
    pub direction: String,
    pub files: Vec<WebDavSyncFile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebDavSyncResult {
    pub direction: String,
    pub global_version: u64,
    pub sync_id: String,
    pub files: Vec<WebDavSyncFile>,
    pub backup_path: String,
    pub marker_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebDavSyncStatus {
    pub local: Option<WebDavSyncMarker>,
    pub remote: Option<WebDavSyncMarker>,
}

#[derive(Debug, Clone, Default)]
pub struct WebDavSyncOptions {
    pub url: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub password_env: Option<String>,
    pub config_path: Option<PathBuf>,
    /// Password for encrypting the sync backup before upload.
    /// If set, each file is encrypted with AES-256-GCM before being
    /// sent to the WebDAV server, and decrypted after download.
    /// If None, files are uploaded as plaintext (backward compatible).
    pub sync_password: Option<String>,
    pub sync_password_env: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct WebDavConfigFile {
    url: Option<String>,
    username: Option<String>,
    password: Option<String>,
    password_env: Option<String>,
    #[serde(default)]
    sync_password: Option<String>,
    #[serde(default)]
    sync_password_env: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedWebDavConfig {
    url: String,
    username: Option<String>,
    password: Option<String>,
    sync_password: Option<String>,
}

fn default_config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("webdav.toml"))
}

fn load_config_file(path: Option<&Path>) -> Result<WebDavConfigFile> {
    let path = match path {
        Some(path) => path.to_path_buf(),
        None => default_config_path()?,
    };
    if !path.exists() {
        return Ok(WebDavConfigFile::default());
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read WebDAV config {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn resolve_config(options: &WebDavSyncOptions) -> Result<ResolvedWebDavConfig> {
    let file = load_config_file(options.config_path.as_deref())?;
    let url = options
        .url
        .clone()
        .or_else(|| std::env::var("AGENT2SSH_WEBDAV_URL").ok())
        .or(file.url)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("WebDAV URL is required (--url or AGENT2SSH_WEBDAV_URL)"))?;
    let username = options
        .username
        .clone()
        .or_else(|| std::env::var("AGENT2SSH_WEBDAV_USERNAME").ok())
        .or(file.username)
        .filter(|value| !value.trim().is_empty());
    let password_env = options
        .password_env
        .clone()
        .or(file.password_env)
        .filter(|value| !value.trim().is_empty());
    let password = options
        .password
        .clone()
        .or_else(|| std::env::var("AGENT2SSH_WEBDAV_PASSWORD").ok())
        .or(file.password)
        .or_else(|| password_env.and_then(|name| std::env::var(name).ok()))
        .filter(|value| !value.is_empty());

    let sync_password_env = options
        .sync_password_env
        .clone()
        .or(file.sync_password_env)
        .filter(|value| !value.trim().is_empty());
    let sync_password = options
        .sync_password
        .clone()
        .or_else(|| std::env::var("AGENT2SSH_SYNC_PASSWORD").ok())
        .or(file.sync_password)
        .or_else(|| sync_password_env.and_then(|name| std::env::var(name).ok()))
        .filter(|value| !value.is_empty());

    Ok(ResolvedWebDavConfig {
        url: url.trim_end_matches('/').to_string(),
        username,
        password,
        sync_password,
    })
}

fn client() -> Result<Client> {
    Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .context("failed to build WebDAV HTTP client")
}

fn join_url(base: &str, child: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        child.trim_start_matches('/')
    )
}

fn with_auth(request: RequestBuilder, config: &ResolvedWebDavConfig) -> RequestBuilder {
    match (&config.username, &config.password) {
        (Some(username), password) => request.basic_auth(username, password.clone()),
        _ => request,
    }
}

async fn ensure_collection(
    client: &Client,
    config: &ResolvedWebDavConfig,
    url: &str,
) -> Result<()> {
    let method = Method::from_bytes(b"MKCOL").expect("valid WebDAV method");
    let response = with_auth(client.request(method, url), config)
        .send()
        .await
        .with_context(|| format!("failed to create WebDAV collection {url}"))?;
    match response.status() {
        status if status.is_success() => Ok(()),
        StatusCode::METHOD_NOT_ALLOWED | StatusCode::CONFLICT => Ok(()),
        status => {
            let body = response.text().await.unwrap_or_default();
            Err(anyhow!(
                "WebDAV MKCOL failed for {url}: {status} {}",
                body.trim()
            ))
        }
    }
}

async fn get_remote_marker(
    client: &Client,
    config: &ResolvedWebDavConfig,
) -> Result<Option<WebDavSyncMarker>> {
    let url = join_url(&config.url, SYNC_VERSION_FILE);
    let response = with_auth(client.get(url.clone()), config)
        .send()
        .await
        .with_context(|| format!("failed to fetch remote marker {url}"))?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "WebDAV GET marker failed: {status} {}",
            body.trim()
        ));
    }
    let bytes = response.bytes().await?;
    serde_json::from_slice(&bytes).context("failed to parse remote sync marker")
}

fn local_marker_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(SYNC_VERSION_FILE))
}

pub fn load_local_sync_marker() -> Result<Option<WebDavSyncMarker>> {
    let path = local_marker_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read local sync marker {}", path.display()))?;
    serde_json::from_str(&raw).context("failed to parse local sync marker")
}

fn write_local_marker(marker: &WebDavSyncMarker) -> Result<PathBuf> {
    let path = local_marker_path()?;
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    fs::write(&tmp, serde_json::to_string_pretty(marker)?)
        .with_context(|| format!("failed to write temp marker {}", tmp.display()))?;
    restrict_file_to_owner(&tmp)?;
    fs::rename(&tmp, &path).with_context(|| format!("failed to replace {}", path.display()))?;
    restrict_file_to_owner(&path)?;
    Ok(path)
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn collect_sync_files() -> Result<Vec<WebDavSyncFile>> {
    let dir = config_dir()?;
    let mut files = Vec::new();
    for rel in SYNCABLE_FILES {
        let path = dir.join(rel);
        if !path.is_file() {
            continue;
        }

        // T1-4: Use CiphertextStore for secrets.enc to enforce read-only access
        // at the type level — the sync fingerprint path must never write secrets.
        if *rel == "secrets.enc" {
            let store = crate::secrets::CiphertextStore::load()?;
            let bytes = store.raw_bytes();
            files.push(WebDavSyncFile {
                path: (*rel).to_string(),
                bytes: bytes.len() as u64,
                sha256: hash_bytes(bytes),
            });
            continue;
        }

        let bytes =
            fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        files.push(WebDavSyncFile {
            path: (*rel).to_string(),
            bytes: bytes.len() as u64,
            sha256: hash_bytes(&bytes),
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn validate_remote_file(path: &str) -> Result<()> {
    if SYNCABLE_FILES.contains(&path) || is_legacy_unsyncable_remote_file(path) {
        Ok(())
    } else {
        Err(anyhow!(
            "remote marker contains unsupported sync file: {path}"
        ))
    }
}

fn is_legacy_unsyncable_remote_file(path: &str) -> bool {
    LEGACY_UNSYNCABLE_REMOTE_FILES.contains(&path)
}

/// `label` is a free-form note for humans browsing the backups directory (e.g.
/// "pre-restore" or a user-supplied snapshot name, V4-3); it does not affect
/// which files get backed up.
pub fn create_sync_backup(label: Option<&str>) -> Result<PathBuf> {
    ensure_config_dir()?;
    let dir = config_dir()?;
    let sync_id = uuid::Uuid::new_v4().simple().to_string();
    let backup = dir.join(BACKUP_DIR).join(format!(
        "sync-{}-{}",
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        &sync_id[..8]
    ));
    fs::create_dir_all(&backup)
        .with_context(|| format!("failed to create backup {}", backup.display()))?;

    let mut copied = Vec::new();
    for rel in SYNCABLE_FILES
        .iter()
        .copied()
        .chain(std::iter::once(SYNC_VERSION_FILE))
    {
        let source = dir.join(rel);
        if !source.is_file() {
            continue;
        }
        let dest = backup.join(rel);
        fs::copy(&source, &dest).with_context(|| {
            format!(
                "failed to back up {} to {}",
                source.display(),
                dest.display()
            )
        })?;
        restrict_file_to_owner(&dest)?;
        copied.push((*rel).to_string());
    }

    let backup_manifest = serde_json::json!({
        "created_at": Utc::now(),
        "source": dir,
        "files": copied,
        "label": label,
    });
    let manifest_path = backup.join("backup_manifest.json");
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&backup_manifest)?,
    )?;
    restrict_file_to_owner(&manifest_path)?;
    Ok(backup)
}

/// V4-3: metadata for a config backup/snapshot, read back from the
/// `backup_manifest.json` that `create_sync_backup` already writes.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigSnapshotInfo {
    pub id: String,
    pub label: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub files: Vec<String>,
}

fn read_backup_manifest(dir: &Path) -> Result<ConfigSnapshotInfo> {
    let manifest_path = dir.join("backup_manifest.json");
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
    let id = dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    Ok(ConfigSnapshotInfo {
        id,
        label: value
            .get("label")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        created_at: value
            .get("created_at")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc)),
        files: value
            .get("files")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|f| f.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// V4-3: list config snapshots under `~/.agent2ssh/backups/`, newest first.
/// Every push/pull sync already lands a backup here (see `create_sync_backup`
/// call sites); this just makes that directory browsable/restorable from the
/// desktop UI instead of being a purely internal safety net.
pub fn list_config_snapshots() -> Result<Vec<ConfigSnapshotInfo>> {
    let dir = config_dir()?.join(BACKUP_DIR);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut snapshots = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        if !entry.path().is_dir() {
            continue;
        }
        if let Ok(info) = read_backup_manifest(&entry.path()) {
            snapshots.push(info);
        }
    }
    snapshots.sort_by_key(|snapshot| std::cmp::Reverse(snapshot.created_at));
    Ok(snapshots)
}

fn backup_dir_for_id(id: &str) -> Result<PathBuf> {
    // `id` becomes a path component below — reject traversal/separators
    // instead of trusting it, since it comes from the frontend.
    if id.is_empty() || id.contains(['/', '\\']) || id.contains("..") {
        return Err(anyhow!("invalid snapshot id"));
    }
    Ok(config_dir()?.join(BACKUP_DIR).join(id))
}

/// V4-3: create a labeled, on-demand snapshot (as opposed to the automatic
/// pre-sync/pre-restore/pre-template ones).
pub fn create_named_snapshot(label: &str) -> Result<ConfigSnapshotInfo> {
    let dir = create_sync_backup(Some(label))?;
    read_backup_manifest(&dir)
}

/// V4-3: restore a config snapshot. Takes a fresh "pre-restore" safety
/// snapshot of the CURRENT state first (so restoring is itself undoable), then
/// copies back only the files the target snapshot actually captured.
pub fn restore_config_snapshot(id: &str) -> Result<ConfigSnapshotInfo> {
    let source_dir = backup_dir_for_id(id)?;
    let info = read_backup_manifest(&source_dir)?;
    create_sync_backup(Some("pre-restore"))?;
    let dest_dir = config_dir()?;
    for rel in &info.files {
        let source = source_dir.join(rel);
        if !source.is_file() {
            continue;
        }
        let dest = dest_dir.join(rel);
        fs::copy(&source, &dest).with_context(|| {
            format!(
                "failed to restore {} to {}",
                source.display(),
                dest.display()
            )
        })?;
        restrict_file_to_owner(&dest)?;
    }
    Ok(info)
}

pub fn delete_config_snapshot(id: &str) -> Result<()> {
    let dir = backup_dir_for_id(id)?;
    if dir.is_dir() {
        fs::remove_dir_all(&dir).with_context(|| format!("failed to delete {}", dir.display()))?;
    }
    Ok(())
}

/// V4-3: apply a config template — write a fixed set of known-safe files
/// (must already be in `SYNCABLE_FILES`) directly into the config dir, after
/// taking a safety snapshot so the change is undoable via snapshot restore.
pub fn apply_config_template(files: &[(String, String)]) -> Result<ConfigSnapshotInfo> {
    for (name, _) in files {
        if !SYNCABLE_FILES.contains(&name.as_str()) {
            return Err(anyhow!(
                "refusing to write non-syncable config file: {name}"
            ));
        }
    }
    let pre_apply = create_named_snapshot("pre-template")?;
    ensure_config_dir()?;
    let dir = config_dir()?;
    for (name, content) in files {
        let dest = dir.join(name);
        fs::write(&dest, content).with_context(|| format!("failed to write {}", dest.display()))?;
        restrict_file_to_owner(&dest)?;
    }
    Ok(pre_apply)
}

fn sync_lock() -> Result<FileLockGuard> {
    lock_config_file(".sync.lock")
}

fn next_global_version(remote: Option<&WebDavSyncMarker>, local: Option<&WebDavSyncMarker>) -> u64 {
    remote
        .map(|m| m.global_version)
        .into_iter()
        .chain(local.map(|m| m.global_version))
        .max()
        .unwrap_or(0)
        + 1
}

fn marker(global_version: u64, direction: &str, files: Vec<WebDavSyncFile>) -> WebDavSyncMarker {
    WebDavSyncMarker {
        schema_version: 1,
        global_version,
        sync_id: uuid::Uuid::new_v4().to_string(),
        updated_at: Utc::now(),
        app_version: SYNC_MARKER_CLIENT_VERSION.to_string(),
        direction: direction.to_string(),
        files,
    }
}

async fn put_bytes(
    client: &Client,
    config: &ResolvedWebDavConfig,
    url: &str,
    bytes: Vec<u8>,
) -> Result<()> {
    let response = with_auth(client.put(url.to_string()).body(bytes), config)
        .send()
        .await
        .with_context(|| format!("failed to upload {url}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "WebDAV PUT failed for {url}: {status} {}",
            body.trim()
        ));
    }
    Ok(())
}

async fn get_bytes(client: &Client, config: &ResolvedWebDavConfig, url: &str) -> Result<Vec<u8>> {
    let response = with_auth(client.get(url.to_string()), config)
        .send()
        .await
        .with_context(|| format!("failed to download {url}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "WebDAV GET failed for {url}: {status} {}",
            body.trim()
        ));
    }
    Ok(response.bytes().await?.to_vec())
}

pub async fn webdav_push(options: WebDavSyncOptions) -> Result<WebDavSyncResult> {
    let _guard = sync_lock()?;
    let config = resolve_config(&options)?;
    let client = client()?;
    ensure_collection(&client, &config, &config.url).await?;
    ensure_collection(&client, &config, &join_url(&config.url, REMOTE_FILES_DIR)).await?;

    let backup = create_sync_backup(None)?;
    let remote_marker = get_remote_marker(&client, &config).await?;
    let local_marker = load_local_sync_marker()?;
    let files = collect_sync_files()?;
    let marker = marker(
        next_global_version(remote_marker.as_ref(), local_marker.as_ref()),
        "push",
        files.clone(),
    );

    let dir = config_dir()?;
    for file in &files {
        let bytes = fs::read(dir.join(&file.path))?;
        let url = join_url(&join_url(&config.url, REMOTE_FILES_DIR), &file.path);
        let upload_bytes = if let Some(ref pw) = config.sync_password {
            crate::backup_crypto::encrypt_backup(pw.as_bytes(), &bytes)
                .with_context(|| format!("failed to encrypt {}", file.path))?
        } else {
            bytes
        };
        put_bytes(&client, &config, &url, upload_bytes).await?;
    }
    let marker_bytes = serde_json::to_vec_pretty(&marker)?;
    put_bytes(
        &client,
        &config,
        &join_url(&config.url, SYNC_VERSION_FILE),
        marker_bytes,
    )
    .await?;
    let marker_path = write_local_marker(&marker)?;

    Ok(WebDavSyncResult {
        direction: "push".into(),
        global_version: marker.global_version,
        sync_id: marker.sync_id,
        files,
        backup_path: backup.display().to_string(),
        marker_path: marker_path.display().to_string(),
    })
}

pub async fn webdav_pull(options: WebDavSyncOptions) -> Result<WebDavSyncResult> {
    let _guard = sync_lock()?;
    let config = resolve_config(&options)?;
    let client = client()?;
    let remote_marker = get_remote_marker(&client, &config)
        .await?
        .ok_or_else(|| anyhow!("remote WebDAV sync marker does not exist; run push first"))?;
    for file in &remote_marker.files {
        validate_remote_file(&file.path)?;
    }
    let applied_files: Vec<WebDavSyncFile> = remote_marker
        .files
        .iter()
        .filter(|file| !is_legacy_unsyncable_remote_file(&file.path))
        .cloned()
        .collect();

    let backup = create_sync_backup(None)?;
    let dir = config_dir()?;
    let remote_paths: HashSet<String> =
        applied_files.iter().map(|file| file.path.clone()).collect();

    for file in &applied_files {
        let url = join_url(&join_url(&config.url, REMOTE_FILES_DIR), &file.path);
        let bytes = get_bytes(&client, &config, &url).await?;
        let plaintext = if let Some(ref pw) = config.sync_password {
            if crate::backup_crypto::is_encrypted_backup(&bytes) {
                crate::backup_crypto::decrypt_backup(pw.as_bytes(), &bytes)
                    .with_context(|| format!("failed to decrypt {}", file.path))?
            } else {
                // Backward compat: file was pushed before encryption was enabled
                bytes
            }
        } else {
            bytes
        };
        let sha256 = hash_bytes(&plaintext);
        if sha256 != file.sha256 {
            return Err(anyhow!(
                "checksum mismatch for {} (expected {}, got {})",
                file.path,
                file.sha256,
                sha256
            ));
        }
        let dest = dir.join(&file.path);
        let tmp = dest.with_extension(format!("tmp.{}", std::process::id()));
        fs::write(&tmp, plaintext)
            .with_context(|| format!("failed to write {}", tmp.display()))?;
        restrict_file_to_owner(&tmp)?;
        fs::rename(&tmp, &dest).with_context(|| format!("failed to replace {}", dest.display()))?;
        restrict_file_to_owner(&dest)?;
    }

    for rel in SYNCABLE_FILES {
        if remote_paths.contains(*rel) {
            continue;
        }
        let local = dir.join(rel);
        if local.exists() {
            fs::remove_file(&local).with_context(|| {
                format!("failed to remove stale local file {}", local.display())
            })?;
        }
    }

    let local_marker = WebDavSyncMarker {
        direction: "pull".into(),
        sync_id: uuid::Uuid::new_v4().to_string(),
        updated_at: Utc::now(),
        app_version: SYNC_MARKER_CLIENT_VERSION.to_string(),
        files: applied_files.clone(),
        ..remote_marker.clone()
    };
    let marker_path = write_local_marker(&local_marker)?;

    Ok(WebDavSyncResult {
        direction: "pull".into(),
        global_version: remote_marker.global_version,
        sync_id: local_marker.sync_id,
        files: applied_files,
        backup_path: backup.display().to_string(),
        marker_path: marker_path.display().to_string(),
    })
}

pub async fn webdav_status(options: WebDavSyncOptions) -> Result<WebDavSyncStatus> {
    let config = resolve_config(&options)?;
    let client = client()?;
    let local = load_local_sync_marker()?;
    let remote = get_remote_marker(&client, &config).await?;
    Ok(WebDavSyncStatus { local, remote })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn collect_sync_files_excludes_local_tokens_logs_and_keys() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-sync-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("keys")).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);
        fs::write(dir.join("hosts.json"), "{}").unwrap();
        fs::write(dir.join("secrets.enc"), "encrypted").unwrap();
        fs::write(dir.join("known_hosts.json"), "{}").unwrap();
        fs::write(dir.join("daemon.token"), "local-token").unwrap();
        fs::write(dir.join("audit.jsonl"), "{}\n").unwrap();
        fs::write(dir.join("keys").join("id_ed25519"), "private-key").unwrap();

        let files = collect_sync_files().unwrap();
        let names: Vec<_> = files.iter().map(|file| file.path.as_str()).collect();
        assert!(names.contains(&"hosts.json"));
        assert!(names.contains(&"secrets.enc"));
        assert!(!names.contains(&"known_hosts.json"));
        assert!(!names.contains(&"daemon.token"));
        assert!(!names.contains(&"audit.jsonl"));
        assert!(!names.contains(&"keys/id_ed25519"));

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn sync_backup_copies_syncable_files_and_marker_only() {
        let dir =
            std::env::temp_dir().join(format!("agent2ssh-sync-backup-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);
        fs::write(dir.join("hosts.json"), "{}").unwrap();
        fs::write(dir.join("known_hosts.json"), "{}").unwrap();
        fs::write(dir.join(SYNC_VERSION_FILE), "{}").unwrap();
        fs::write(dir.join("daemon.token"), "local-token").unwrap();

        let backup = create_sync_backup(None).unwrap();
        assert!(backup.join("hosts.json").exists());
        assert!(!backup.join("known_hosts.json").exists());
        assert!(backup.join(SYNC_VERSION_FILE).exists());
        assert!(!backup.join("daemon.token").exists());
        assert!(backup.join("backup_manifest.json").exists());

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn next_global_version_uses_highest_known_marker() {
        let local = marker(7, "push", vec![]);
        let remote = marker(12, "push", vec![]);
        assert_eq!(next_global_version(Some(&remote), Some(&local)), 13);
        assert_eq!(next_global_version(None, Some(&local)), 8);
        assert_eq!(next_global_version(None, None), 1);
    }

    #[test]
    fn remote_validation_allows_legacy_known_hosts_but_rejects_unknown_files() {
        validate_remote_file("known_hosts.json").unwrap();
        validate_remote_file("hosts.json").unwrap();
        assert!(validate_remote_file("daemon.token").is_err());
    }

    #[test]
    #[serial_test::serial]
    fn named_snapshot_round_trips_through_list_and_restore() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-snap-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);
        fs::write(dir.join("hosts.json"), r#"{"v":1}"#).unwrap();

        let created = create_named_snapshot("before-change").unwrap();
        assert_eq!(created.label.as_deref(), Some("before-change"));
        assert!(created.files.contains(&"hosts.json".to_string()));

        let listed = list_config_snapshots().unwrap();
        assert!(listed.iter().any(|s| s.id == created.id));

        // Mutate the live file, then restore the snapshot and confirm it's back.
        fs::write(dir.join("hosts.json"), r#"{"v":2}"#).unwrap();
        restore_config_snapshot(&created.id).unwrap();
        let restored = fs::read_to_string(dir.join("hosts.json")).unwrap();
        assert_eq!(restored, r#"{"v":1}"#);

        // Restoring itself must have taken a pre-restore safety snapshot.
        let after_restore = list_config_snapshots().unwrap();
        assert!(after_restore
            .iter()
            .any(|s| s.label.as_deref() == Some("pre-restore")));

        delete_config_snapshot(&created.id).unwrap();
        let after_delete = list_config_snapshots().unwrap();
        assert!(!after_delete.iter().any(|s| s.id == created.id));

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn apply_config_template_rejects_non_syncable_files() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-tmpl-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);

        let result = apply_config_template(&[("daemon.token".to_string(), "x".to_string())]);
        assert!(result.is_err());
        assert!(!dir.join("daemon.token").exists());

        let ok = apply_config_template(&[(
            "execution_limits.toml".to_string(),
            "enabled = true\n".to_string(),
        )]);
        assert!(ok.is_ok());
        assert_eq!(
            fs::read_to_string(dir.join("execution_limits.toml")).unwrap(),
            "enabled = true\n"
        );

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn backup_dir_for_id_rejects_traversal() {
        assert!(backup_dir_for_id("../etc").is_err());
        assert!(backup_dir_for_id("a/b").is_err());
        assert!(backup_dir_for_id("").is_err());
    }
}
