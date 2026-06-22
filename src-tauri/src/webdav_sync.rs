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
}

#[derive(Debug, Clone, Deserialize, Default)]
struct WebDavConfigFile {
    url: Option<String>,
    username: Option<String>,
    password: Option<String>,
    password_env: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedWebDavConfig {
    url: String,
    username: Option<String>,
    password: Option<String>,
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

    Ok(ResolvedWebDavConfig {
        url: url.trim_end_matches('/').to_string(),
        username,
        password,
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
    if SYNCABLE_FILES.iter().any(|item| *item == path) || is_legacy_unsyncable_remote_file(path) {
        Ok(())
    } else {
        Err(anyhow!(
            "remote marker contains unsupported sync file: {path}"
        ))
    }
}

fn is_legacy_unsyncable_remote_file(path: &str) -> bool {
    LEGACY_UNSYNCABLE_REMOTE_FILES
        .iter()
        .any(|item| *item == path)
}

pub fn create_sync_backup() -> Result<PathBuf> {
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
    });
    let manifest_path = backup.join("backup_manifest.json");
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&backup_manifest)?,
    )?;
    restrict_file_to_owner(&manifest_path)?;
    Ok(backup)
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

    let backup = create_sync_backup()?;
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
        put_bytes(&client, &config, &url, bytes).await?;
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

    let backup = create_sync_backup()?;
    let dir = config_dir()?;
    let remote_paths: HashSet<String> =
        applied_files.iter().map(|file| file.path.clone()).collect();

    for file in &applied_files {
        let url = join_url(&join_url(&config.url, REMOTE_FILES_DIR), &file.path);
        let bytes = get_bytes(&client, &config, &url).await?;
        let sha256 = hash_bytes(&bytes);
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
        fs::write(&tmp, bytes).with_context(|| format!("failed to write {}", tmp.display()))?;
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

        let backup = create_sync_backup().unwrap();
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
}
