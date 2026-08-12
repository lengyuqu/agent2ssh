use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use reqwest::{Client, Method, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    future::Future,
    path::{Path, PathBuf},
};

use crate::store::{
    config_dir, ensure_config_dir, lock_config_file, restrict_file_to_owner, FileLockGuard,
};

const SYNC_VERSION_FILE: &str = "sync_version.json";
const BACKUP_DIR: &str = "backups";
const REMOTE_FILES_DIR: &str = "files";
const REMOTE_VERSIONS_DIR: &str = "versions";
const SYNC_MARKER_CLIENT_VERSION: &str = "redacted";
const CURRENT_SYNC_SCHEMA: u32 = 2;

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
    "snippets.json",
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
    /// Stable digest of the portable configuration represented by `files`.
    /// Older markers did not include this field; their digest is derived from
    /// the file manifest when read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    /// Prefix for immutable version objects. Schema v1 omitted this and used
    /// the mutable `files/` directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_prefix: Option<String>,
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
    pub state: SyncState,
    pub local_digest: Option<String>,
    pub remote_digest: Option<String>,
    pub base_digest: Option<String>,
    pub summary: String,
    pub metadata_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    InSync,
    LocalAhead,
    RemoteAhead,
    Diverged,
    Unknown,
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
    /// Explicitly allow overwriting configuration when the remote and local
    /// sides no longer share the same last-applied digest.
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct RemoteObject {
    pub bytes: Vec<u8>,
    /// Opaque backend version (an ETag for WebDAV).
    pub version: Option<String>,
}

#[derive(Debug, Clone)]
pub enum RemoteWriteCondition {
    Any,
    Missing,
    Version(String),
}

/// Transport boundary for portable configuration synchronization. The sync
/// algorithm deliberately knows nothing about WebDAV URLs or authentication,
/// which makes additional backends and deterministic conflict tests possible.
pub trait SyncRemote {
    fn ensure_layout(&self) -> impl Future<Output = Result<()>> + Send;
    fn read(&self, path: &str) -> impl Future<Output = Result<Option<RemoteObject>>> + Send;
    fn write(
        &self,
        path: &str,
        bytes: Vec<u8>,
        condition: RemoteWriteCondition,
    ) -> impl Future<Output = Result<()>> + Send;
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

#[derive(Clone)]
struct WebDavRemote {
    client: Client,
    config: ResolvedWebDavConfig,
}

impl WebDavRemote {
    fn url(&self, path: &str) -> String {
        join_url(&self.config.url, path)
    }
}

impl SyncRemote for WebDavRemote {
    async fn ensure_layout(&self) -> Result<()> {
        ensure_collection(&self.client, &self.config, &self.config.url).await?;
        ensure_collection(&self.client, &self.config, &self.url(REMOTE_FILES_DIR)).await?;
        ensure_collection(&self.client, &self.config, &self.url(REMOTE_VERSIONS_DIR)).await
    }

    async fn read(&self, path: &str) -> Result<Option<RemoteObject>> {
        let url = self.url(path);
        let response = with_auth(self.client.get(url.clone()), &self.config)
            .send()
            .await
            .with_context(|| format!("failed to fetch remote object {url}"))?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "WebDAV GET failed for {url}: {status} {}",
                body.trim()
            ));
        }
        let version = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        Ok(Some(RemoteObject {
            bytes: response.bytes().await?.to_vec(),
            version,
        }))
    }

    async fn write(
        &self,
        path: &str,
        bytes: Vec<u8>,
        condition: RemoteWriteCondition,
    ) -> Result<()> {
        let url = self.url(path);
        let mut request = with_auth(self.client.put(url.clone()).body(bytes), &self.config);
        request = match condition {
            RemoteWriteCondition::Any => request,
            RemoteWriteCondition::Missing => request.header(reqwest::header::IF_NONE_MATCH, "*"),
            RemoteWriteCondition::Version(version) => {
                request.header(reqwest::header::IF_MATCH, version)
            }
        };
        let response = request
            .send()
            .await
            .with_context(|| format!("failed to upload {url}"))?;
        if response.status() == StatusCode::PRECONDITION_FAILED
            || response.status() == StatusCode::CONFLICT
        {
            return Err(anyhow!(
                "sync conflict: remote object changed concurrently while writing {path}"
            ));
        }
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
}

async fn get_remote_marker<R: SyncRemote>(
    remote: &R,
) -> Result<Option<(WebDavSyncMarker, Option<String>)>> {
    let Some(object) = remote.read(SYNC_VERSION_FILE).await? else {
        return Ok(None);
    };
    let marker =
        serde_json::from_slice(&object.bytes).context("failed to parse remote sync marker")?;
    validate_marker_schema(&marker)?;
    Ok(Some((marker, object.version)))
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

/// Return a stable, order-independent SHA-256 digest for a portable config
/// manifest. Timestamps, sync ids, and backend metadata are intentionally not
/// included, so identical configuration has the same digest on every device.
pub fn portable_config_digest(files: &[WebDavSyncFile]) -> String {
    let mut ordered = files.to_vec();
    ordered.sort_by(|a, b| a.path.cmp(&b.path));
    let mut hasher = Sha256::new();
    hasher.update(b"agent2ssh-portable-config-v1\0");
    for file in ordered {
        hasher.update(file.path.as_bytes());
        hasher.update(b"\0");
        hasher.update(file.sha256.as_bytes());
        hasher.update(b"\0");
    }
    hex::encode(hasher.finalize())
}

pub fn current_portable_config_digest() -> Result<String> {
    Ok(portable_config_digest(&collect_sync_files()?))
}

pub fn sync_marker_digest(marker: &WebDavSyncMarker) -> String {
    marker
        .digest
        .clone()
        .unwrap_or_else(|| portable_config_digest(&marker.files))
}

fn classify_sync_state(
    local_digest: &str,
    local_marker: Option<&WebDavSyncMarker>,
    remote_marker: Option<&WebDavSyncMarker>,
) -> SyncState {
    let Some(remote) = remote_marker else {
        return SyncState::LocalAhead;
    };
    let remote_digest = sync_marker_digest(remote);
    if remote_digest == local_digest {
        return SyncState::InSync;
    }
    let Some(local) = local_marker else {
        return if local_digest == portable_config_digest(&[]) {
            SyncState::RemoteAhead
        } else {
            SyncState::Unknown
        };
    };
    let base_digest = sync_marker_digest(local);
    if local_digest == base_digest {
        if remote.global_version > local.global_version {
            SyncState::RemoteAhead
        } else {
            // The remote regressed or was replaced at the same version. Do
            // not silently bless either side as newer.
            SyncState::Diverged
        }
    } else if remote_digest == base_digest {
        SyncState::LocalAhead
    } else {
        SyncState::Diverged
    }
}

fn sync_summary(state: SyncState) -> &'static str {
    match state {
        SyncState::InSync => "Local and remote portable configuration are in sync.",
        SyncState::LocalAhead => "Local portable configuration has changes not present remotely.",
        SyncState::RemoteAhead => "Remote portable configuration has changes not applied locally.",
        SyncState::Diverged => "Local and remote portable configuration have diverged.",
        SyncState::Unknown => {
            "Sync relationship is unknown because no trustworthy common metadata is available."
        }
    }
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
        // S1: Defense-in-depth — reject path traversal even for whitelisted names
        if !crate::keys::is_safe_filename(name) {
            return Err(anyhow!(
                "refusing to write config file with unsafe name: {name}"
            ));
        }
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
    let digest = portable_config_digest(&files);
    let sync_id = uuid::Uuid::new_v4().to_string();
    WebDavSyncMarker {
        schema_version: CURRENT_SYNC_SCHEMA,
        global_version,
        object_prefix: Some(format!("{REMOTE_VERSIONS_DIR}/{sync_id}-")),
        sync_id,
        updated_at: Utc::now(),
        app_version: SYNC_MARKER_CLIENT_VERSION.to_string(),
        direction: direction.to_string(),
        files,
        digest: Some(digest),
    }
}

fn validate_marker_schema(marker: &WebDavSyncMarker) -> Result<()> {
    if marker.schema_version == 0 || marker.schema_version > CURRENT_SYNC_SCHEMA {
        return Err(anyhow!(
            "unsupported sync marker schema {} (this client supports up to {})",
            marker.schema_version,
            CURRENT_SYNC_SCHEMA
        ));
    }
    Ok(())
}

fn remote_object_path(marker: &WebDavSyncMarker, file: &WebDavSyncFile) -> String {
    marker
        .object_prefix
        .as_ref()
        .map(|prefix| format!("{prefix}{}", file.path))
        .unwrap_or_else(|| format!("{REMOTE_FILES_DIR}/{}", file.path))
}

pub async fn webdav_push(options: WebDavSyncOptions) -> Result<WebDavSyncResult> {
    let _guard = sync_lock()?;
    let config = resolve_config(&options)?;
    let remote = WebDavRemote {
        client: client()?,
        config: config.clone(),
    };
    sync_push(&remote, config.sync_password.as_deref(), options.force).await
}

async fn sync_push<R: SyncRemote>(
    remote: &R,
    sync_password: Option<&str>,
    force: bool,
) -> Result<WebDavSyncResult> {
    remote.ensure_layout().await?;

    let remote_info = get_remote_marker(remote).await?;
    let remote_marker = remote_info.as_ref().map(|(marker, _)| marker);
    let local_marker = load_local_sync_marker()?;
    let files = collect_sync_files()?;
    let local_digest = portable_config_digest(&files);
    let state = classify_sync_state(&local_digest, local_marker.as_ref(), remote_marker);
    if !force
        && matches!(
            state,
            SyncState::RemoteAhead | SyncState::Diverged | SyncState::Unknown
        )
    {
        return Err(anyhow!(
            "sync conflict ({state:?}): refusing to overwrite remote configuration; inspect status and retry with --force"
        ));
    }

    let backup = create_sync_backup(None)?;
    let new_marker = marker(
        next_global_version(remote_marker, local_marker.as_ref()),
        "push",
        files.clone(),
    );

    let dir = config_dir()?;
    for file in &files {
        let bytes = fs::read(dir.join(&file.path))?;
        let upload_bytes = if let Some(pw) = sync_password {
            crate::backup_crypto::encrypt_backup(pw.as_bytes(), &bytes)
                .with_context(|| format!("failed to encrypt {}", file.path))?
        } else {
            bytes
        };
        remote
            .write(
                &remote_object_path(&new_marker, file),
                upload_bytes,
                RemoteWriteCondition::Missing,
            )
            .await?;
    }
    let condition = match remote_info {
        None => RemoteWriteCondition::Missing,
        Some((expected, Some(version))) => {
            let _ = expected;
            RemoteWriteCondition::Version(version)
        }
        Some((expected, None)) => {
            // Some older WebDAV servers omit ETags. Re-read immediately before
            // committing the marker and reject a detectable concurrent update.
            let current = get_remote_marker(remote)
                .await?
                .ok_or_else(|| anyhow!("sync conflict: remote marker disappeared"))?;
            if current.0.sync_id != expected.sync_id
                || current.0.global_version != expected.global_version
                || sync_marker_digest(&current.0) != sync_marker_digest(&expected)
            {
                return Err(anyhow!("sync conflict: remote marker changed concurrently"));
            }
            RemoteWriteCondition::Any
        }
    };
    remote
        .write(
            SYNC_VERSION_FILE,
            serde_json::to_vec_pretty(&new_marker)?,
            condition,
        )
        .await?;
    let marker_path = write_local_marker(&new_marker)?;

    Ok(WebDavSyncResult {
        direction: "push".into(),
        global_version: new_marker.global_version,
        sync_id: new_marker.sync_id,
        files,
        backup_path: backup.display().to_string(),
        marker_path: marker_path.display().to_string(),
    })
}

pub async fn webdav_pull(options: WebDavSyncOptions) -> Result<WebDavSyncResult> {
    let _guard = sync_lock()?;
    let config = resolve_config(&options)?;
    let remote = WebDavRemote {
        client: client()?,
        config: config.clone(),
    };
    sync_pull(&remote, config.sync_password.as_deref(), options.force).await
}

async fn sync_pull<R: SyncRemote>(
    remote: &R,
    sync_password: Option<&str>,
    force: bool,
) -> Result<WebDavSyncResult> {
    let (remote_marker, _) = get_remote_marker(remote)
        .await?
        .ok_or_else(|| anyhow!("remote sync marker does not exist; run push first"))?;
    for file in &remote_marker.files {
        validate_remote_file(&file.path)?;
    }
    let applied_files: Vec<WebDavSyncFile> = remote_marker
        .files
        .iter()
        .filter(|file| !is_legacy_unsyncable_remote_file(&file.path))
        .cloned()
        .collect();

    let local_marker = load_local_sync_marker()?;
    let local_digest = current_portable_config_digest()?;
    let state = classify_sync_state(&local_digest, local_marker.as_ref(), Some(&remote_marker));
    if !force
        && matches!(
            state,
            SyncState::LocalAhead | SyncState::Diverged | SyncState::Unknown
        )
    {
        return Err(anyhow!(
            "sync conflict ({state:?}): refusing to overwrite local configuration; inspect status and retry with --force"
        ));
    }

    // Download and verify the complete snapshot before touching local files.
    let mut downloaded = Vec::with_capacity(applied_files.len());
    for file in &applied_files {
        let object = remote
            .read(&remote_object_path(&remote_marker, file))
            .await?
            .ok_or_else(|| anyhow!("remote sync file is missing: {}", file.path))?;
        let plaintext = if let Some(pw) = sync_password {
            if crate::backup_crypto::is_encrypted_backup(&object.bytes) {
                crate::backup_crypto::decrypt_backup(pw.as_bytes(), &object.bytes)
                    .with_context(|| format!("failed to decrypt {}", file.path))?
            } else {
                object.bytes
            }
        } else {
            object.bytes
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
        downloaded.push((file.path.clone(), plaintext));
    }

    let latest = get_remote_marker(remote)
        .await?
        .ok_or_else(|| anyhow!("sync conflict: remote marker disappeared"))?;
    if latest.0.sync_id != remote_marker.sync_id
        || latest.0.global_version != remote_marker.global_version
        || sync_marker_digest(&latest.0) != sync_marker_digest(&remote_marker)
    {
        return Err(anyhow!("sync conflict: remote marker changed during pull"));
    }

    let backup = create_sync_backup(None)?;
    let dir = config_dir()?;
    let remote_paths: HashSet<String> =
        applied_files.iter().map(|file| file.path.clone()).collect();

    for (path, plaintext) in downloaded {
        let dest = dir.join(&path);
        let tmp = dest.with_extension(format!("tmp.{}", std::process::id()));
        fs::write(&tmp, plaintext).with_context(|| format!("failed to write {}", tmp.display()))?;
        restrict_file_to_owner(&tmp)?;
        fs::rename(&tmp, &dest).with_context(|| format!("failed to replace {}", dest.display()))?;
        restrict_file_to_owner(&dest)?;
    }

    for rel in SYNCABLE_FILES {
        // Schema v1 predates snippets. Absence in an old manifest means the
        // old client did not know about the file, not that the user deleted it.
        if remote_marker.schema_version == 1 && *rel == "snippets.json" {
            continue;
        }
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
        schema_version: remote_marker.schema_version.max(2),
        direction: "pull".into(),
        sync_id: uuid::Uuid::new_v4().to_string(),
        updated_at: Utc::now(),
        app_version: SYNC_MARKER_CLIENT_VERSION.to_string(),
        files: applied_files.clone(),
        digest: Some(portable_config_digest(&applied_files)),
        object_prefix: remote_marker.object_prefix.clone(),
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
    let remote_transport = WebDavRemote {
        client: client()?,
        config,
    };
    sync_status(&remote_transport).await
}

async fn sync_status<R: SyncRemote>(remote_transport: &R) -> Result<WebDavSyncStatus> {
    let mut metadata_errors = Vec::new();
    let local = match load_local_sync_marker() {
        Ok(marker) => marker,
        Err(error) => {
            metadata_errors.push(format!("local metadata: {error}"));
            None
        }
    };
    let remote_object = remote_transport.read(SYNC_VERSION_FILE).await?;
    let remote = match remote_object {
        Some(object) => match serde_json::from_slice::<WebDavSyncMarker>(&object.bytes) {
            Ok(marker) if validate_marker_schema(&marker).is_ok() => Some(marker),
            Ok(marker) => {
                metadata_errors.push(format!(
                    "remote metadata: unsupported schema {}",
                    marker.schema_version
                ));
                None
            }
            Err(error) => {
                metadata_errors.push(format!("remote metadata: {error}"));
                None
            }
        },
        None => None,
    };
    let local_digest = current_portable_config_digest()?;
    let state = if metadata_errors.is_empty() {
        classify_sync_state(&local_digest, local.as_ref(), remote.as_ref())
    } else {
        SyncState::Unknown
    };
    let remote_digest = remote.as_ref().map(sync_marker_digest);
    let base_digest = local.as_ref().map(sync_marker_digest);
    let summary = if metadata_errors.is_empty() {
        sync_summary(state).to_string()
    } else {
        format!("{} {}", sync_summary(state), metadata_errors.join("; "))
    };
    Ok(WebDavSyncStatus {
        local,
        remote,
        state,
        local_digest: Some(local_digest),
        remote_digest,
        base_digest,
        summary,
        metadata_error: (!metadata_errors.is_empty()).then(|| metadata_errors.join("; ")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeRemote {
        objects: Mutex<HashMap<String, (Vec<u8>, u64)>>,
        mutate_marker_on_file_write: bool,
    }

    impl FakeRemote {
        fn insert(&self, path: &str, bytes: Vec<u8>) {
            self.objects
                .lock()
                .unwrap()
                .insert(path.to_string(), (bytes, 1));
        }
    }

    impl SyncRemote for FakeRemote {
        async fn ensure_layout(&self) -> Result<()> {
            Ok(())
        }

        async fn read(&self, path: &str) -> Result<Option<RemoteObject>> {
            Ok(self
                .objects
                .lock()
                .unwrap()
                .get(path)
                .map(|(bytes, version)| RemoteObject {
                    bytes: bytes.clone(),
                    version: Some(version.to_string()),
                }))
        }

        async fn write(
            &self,
            path: &str,
            bytes: Vec<u8>,
            condition: RemoteWriteCondition,
        ) -> Result<()> {
            let mut objects = self.objects.lock().unwrap();
            let current = objects.get(path).map(|(_, version)| *version);
            let condition_matches = match condition {
                RemoteWriteCondition::Any => true,
                RemoteWriteCondition::Missing => current.is_none(),
                RemoteWriteCondition::Version(expected) => {
                    current.map(|v| v.to_string()).as_deref() == Some(expected.as_str())
                }
            };
            if !condition_matches {
                return Err(anyhow!("sync conflict: fake remote version changed"));
            }
            objects.insert(path.to_string(), (bytes, current.unwrap_or(0) + 1));
            if self.mutate_marker_on_file_write
                && (path.starts_with("files/") || path.starts_with("versions/"))
            {
                if let Some((_, version)) = objects.get_mut(SYNC_VERSION_FILE) {
                    *version += 1;
                }
            }
            Ok(())
        }
    }

    fn file(path: &str, content: &[u8]) -> WebDavSyncFile {
        WebDavSyncFile {
            path: path.to_string(),
            bytes: content.len() as u64,
            sha256: hash_bytes(content),
        }
    }

    #[test]
    fn portable_digest_is_stable_across_manifest_order_and_metadata() {
        let first = file("hosts.json", b"hosts");
        let mut second = file("policy.toml", b"policy");
        let digest = portable_config_digest(&[first.clone(), second.clone()]);
        second.bytes = 999;
        assert_eq!(
            digest,
            portable_config_digest(&[second.clone(), first.clone()])
        );
        second.sha256 = hash_bytes(b"changed");
        assert_ne!(digest, portable_config_digest(&[first, second]));
    }

    #[test]
    fn old_remote_marker_without_digest_remains_compatible() {
        let raw = r#"{
          "schema_version":1,"global_version":3,"sync_id":"old",
          "updated_at":"2026-01-01T00:00:00Z","app_version":"old",
          "direction":"push","files":[]
        }"#;
        let parsed: WebDavSyncMarker = serde_json::from_str(raw).unwrap();
        assert!(parsed.digest.is_none());
        assert_eq!(sync_marker_digest(&parsed), portable_config_digest(&[]));
    }

    #[test]
    fn sync_state_distinguishes_ahead_and_diverged_snapshots() {
        let base_files = vec![file("hosts.json", b"base")];
        let local_files = vec![file("hosts.json", b"local")];
        let remote_files = vec![file("hosts.json", b"remote")];
        let base = marker(4, "pull", base_files.clone());
        let remote_new = marker(5, "push", remote_files.clone());
        assert_eq!(
            classify_sync_state(
                &portable_config_digest(&base_files),
                Some(&base),
                Some(&remote_new)
            ),
            SyncState::RemoteAhead
        );
        assert_eq!(
            classify_sync_state(
                &portable_config_digest(&local_files),
                Some(&base),
                Some(&base)
            ),
            SyncState::LocalAhead
        );
        assert_eq!(
            classify_sync_state(
                &portable_config_digest(&local_files),
                Some(&base),
                Some(&remote_new)
            ),
            SyncState::Diverged
        );
        assert_eq!(
            classify_sync_state(
                &portable_config_digest(&remote_files),
                Some(&base),
                Some(&remote_new)
            ),
            SyncState::InSync
        );
        assert_eq!(
            classify_sync_state(
                &portable_config_digest(&local_files),
                None,
                Some(&remote_new)
            ),
            SyncState::Unknown
        );
        assert_eq!(
            classify_sync_state(&portable_config_digest(&[]), None, Some(&remote_new)),
            SyncState::RemoteAhead
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn status_degrades_corrupt_metadata_to_unknown() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-sync-bad-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);
        fs::write(dir.join("hosts.json"), "{}").unwrap();
        fs::write(dir.join(SYNC_VERSION_FILE), "not-json").unwrap();
        let remote = FakeRemote::default();
        remote.insert(SYNC_VERSION_FILE, b"also-not-json".to_vec());

        let status = sync_status(&remote).await.unwrap();
        assert_eq!(status.state, SyncState::Unknown);
        assert!(status.metadata_error.unwrap().contains("metadata"));

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn push_rejects_concurrent_remote_marker_change() {
        let dir =
            std::env::temp_dir().join(format!("agent2ssh-sync-race-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);
        let base = marker(1, "push", vec![file("hosts.json", b"base")]);
        write_local_marker(&base).unwrap();
        fs::write(dir.join("hosts.json"), b"local change").unwrap();

        let remote = FakeRemote {
            mutate_marker_on_file_write: true,
            ..FakeRemote::default()
        };
        remote.insert(SYNC_VERSION_FILE, serde_json::to_vec(&base).unwrap());
        let result = sync_push(&remote, None, false).await;
        assert!(result.unwrap_err().to_string().contains("conflict"));

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn legacy_pull_preserves_snippets_missing_from_old_manifest() {
        let dir = std::env::temp_dir().join(format!(
            "agent2ssh-sync-legacy-pull-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);
        fs::write(dir.join("snippets.json"), r#"[{"name":"keep"}]"#).unwrap();

        let host_bytes = b"{\"hosts\":[]}".to_vec();
        let legacy = WebDavSyncMarker {
            schema_version: 1,
            global_version: 1,
            sync_id: "legacy".to_string(),
            updated_at: Utc::now(),
            app_version: "old".to_string(),
            direction: "push".to_string(),
            files: vec![file("hosts.json", &host_bytes)],
            digest: None,
            object_prefix: None,
        };
        let remote = FakeRemote::default();
        remote.insert(SYNC_VERSION_FILE, serde_json::to_vec(&legacy).unwrap());
        remote.insert("files/hosts.json", host_bytes);

        sync_pull(&remote, None, true).await.unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("snippets.json")).unwrap(),
            r#"[{"name":"keep"}]"#
        );

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn future_schema_is_rejected_before_push_writes() {
        let dir =
            std::env::temp_dir().join(format!("agent2ssh-sync-future-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);
        fs::write(dir.join("hosts.json"), "{}").unwrap();
        let mut future = marker(9, "push", vec![]);
        future.schema_version = CURRENT_SYNC_SCHEMA + 1;
        let remote = FakeRemote::default();
        remote.insert(SYNC_VERSION_FILE, serde_json::to_vec(&future).unwrap());

        let result = sync_push(&remote, None, true).await;
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unsupported sync marker schema"));
        assert_eq!(remote.objects.lock().unwrap().len(), 1);

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

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
