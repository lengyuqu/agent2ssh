//! Validation and recovery of the desktop's encrypted JSON snapshot format.
use crate::{
    store::{config_dir, write_private_bytes},
    webdav_sync::{create_sync_backup, restore_config_snapshot, SYNCABLE_FILES},
};
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::{collections::HashSet, fs, path::Path};

pub const MAX_BACKUP_BYTES: usize = 20 * 1024 * 1024;

#[derive(Deserialize)]
pub struct Snapshot {
    version: u32,
    #[serde(rename = "exportedAt")]
    exported_at: String,
    files: Vec<SnapshotFile>,
}

#[derive(Deserialize)]
struct SnapshotFile {
    path: String,
    content: String,
    bytes: u64,
}

impl Drop for SnapshotFile {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.content);
    }
}

pub fn decode(password: &str, encrypted: &[u8]) -> Result<Snapshot> {
    if encrypted.len() > MAX_BACKUP_BYTES {
        bail!("cloud backup exceeds the 20 MiB limit");
    }
    let plaintext = zeroize::Zeroizing::new(crate::backup_crypto::decrypt_backup(
        password.as_bytes(),
        encrypted,
    )?);
    let snapshot: Snapshot =
        serde_json::from_slice(&plaintext).context("invalid desktop backup JSON")?;
    snapshot.validate()?;
    Ok(snapshot)
}

impl Snapshot {
    fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("unsupported desktop backup version: {}", self.version);
        }
        chrono::DateTime::parse_from_rfc3339(&self.exported_at)
            .context("invalid backup timestamp")?;
        if self.files.is_empty() {
            bail!("desktop backup contains no configuration files");
        }
        let mut names = HashSet::new();
        for file in &self.files {
            if !SYNCABLE_FILES.contains(&file.path.as_str()) || !names.insert(&file.path) {
                bail!("unsupported or duplicate backup file: {}", file.path);
            }
            if file.bytes != file.content.len() as u64 {
                bail!("backup file size mismatch: {}", file.path);
            }
            if file.path.ends_with(".json") {
                let _: serde_json::Value = serde_json::from_str(&file.content)
                    .with_context(|| format!("invalid JSON in {}", file.path))?;
                if file.path == "hosts.json" {
                    let _: crate::types::AppConfig = serde_json::from_str(&file.content)
                        .context("invalid host profiles in backup")?;
                }
            } else {
                let _: toml::Value = toml::from_str(&file.content)
                    .with_context(|| format!("invalid TOML in {}", file.path))?;
            }
        }
        Ok(())
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Returns the undo snapshot id. Failures after the first write roll back
    /// through the same local snapshot mechanism exposed by the desktop.
    pub fn restore(&self) -> Result<String> {
        self.restore_with(write_private_bytes)
    }

    fn restore_with(&self, mut write: impl FnMut(&Path, &[u8]) -> Result<()>) -> Result<String> {
        self.validate()?;
        let dir = config_dir()?;
        let scope: Vec<_> = SYNCABLE_FILES
            .iter()
            .copied()
            .chain(std::iter::once("sync_version.json"))
            .collect();
        for name in &scope {
            match fs::symlink_metadata(dir.join(name)) {
                Ok(meta) if !meta.file_type().is_file() => {
                    bail!("cannot restore over non-regular configuration file: {name}")
                }
                Ok(meta) if meta.permissions().readonly() => {
                    bail!("cannot restore read-only configuration file: {name}")
                }
                Ok(_) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.into()),
            }
        }
        let backup = create_sync_backup(Some("pre-cloud-restore"))?;
        let id = backup
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("invalid recovery snapshot id"))?
            .to_owned();
        let applied = (|| -> Result<()> {
            for file in &self.files {
                write(&dir.join(&file.path), file.content.as_bytes())?;
            }
            for name in &scope {
                if !self.files.iter().any(|f| f.path == *name) && dir.join(name).exists() {
                    fs::remove_file(dir.join(name))?;
                }
            }
            Ok(())
        })();
        if let Err(error) = applied {
            return match restore_config_snapshot(&id) {
                Ok(_) => Err(anyhow!("cloud restore failed; previous configuration restored: {error:#}")),
                Err(rollback) => Err(anyhow!("cloud restore failed: {error:#}; rollback failed: {rollback:#}; recovery snapshot: {id}")),
            };
        }
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(files: &[(&str, &str)]) -> Snapshot {
        serde_json::from_value(serde_json::json!({"version":1,"exportedAt":"2026-10-23T00:00:00Z","files":files.iter().map(|(path, content)|serde_json::json!({"path":path,"content":content,"bytes":content.len()})).collect::<Vec<_>>()})).unwrap()
    }

    #[test]
    fn restore_replaces_the_portable_set_and_has_an_undo_snapshot() {
        let dir = crate::store::TestConfigDir::new("cloud-restore-undo");
        fs::write(dir.join("policy.toml"), "mode = 'old'").unwrap();
        fs::write(dir.join("anomaly.toml"), "enabled = true").unwrap();
        fs::write(dir.join("sync_version.json"), "{}").unwrap();
        fs::write(dir.join("secrets.enc"), "local-vault").unwrap();
        fs::write(dir.join("known_hosts.json"), "trust").unwrap();
        fs::create_dir_all(dir.join("keys")).unwrap();
        fs::write(dir.join("keys/private"), "key").unwrap();
        let id = snapshot(&[("policy.toml", "mode = 'new'")])
            .restore()
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("policy.toml")).unwrap(),
            "mode = 'new'"
        );
        assert!(!dir.join("anomaly.toml").exists());
        assert!(!dir.join("sync_version.json").exists());
        assert_eq!(
            fs::read_to_string(dir.join("secrets.enc")).unwrap(),
            "local-vault"
        );
        assert_eq!(
            fs::read_to_string(dir.join("known_hosts.json")).unwrap(),
            "trust"
        );
        assert_eq!(fs::read_to_string(dir.join("keys/private")).unwrap(), "key");
        restore_config_snapshot(&id).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("policy.toml")).unwrap(),
            "mode = 'old'"
        );
        assert!(dir.join("anomaly.toml").exists());
    }

    #[test]
    fn invalid_snapshots_do_not_touch_local_configuration() {
        let dir = crate::store::TestConfigDir::new("cloud-invalid");
        fs::write(dir.join("policy.toml"), "old").unwrap();
        for files in [
            vec![],
            vec![("../outside", "x")],
            vec![("secrets.enc", "x")],
            vec![("policy.toml", "invalid = [")],
            vec![("hosts.json", "{\"hosts\":false}")],
            vec![("policy.toml", "x=1"), ("policy.toml", "x=2")],
        ] {
            assert!(snapshot(&files).restore().is_err());
            assert_eq!(fs::read_to_string(dir.join("policy.toml")).unwrap(), "old");
        }
        let mut bad = snapshot(&[("policy.toml", "x=1")]);
        bad.files[0].bytes += 1;
        assert!(bad.restore().is_err());
        bad.version = 999;
        assert!(bad.restore().is_err());
    }

    #[test]
    fn a_partial_write_failure_rolls_back_all_configuration() {
        let dir = crate::store::TestConfigDir::new("cloud-rollback");
        fs::write(dir.join("policy.toml"), "mode = 'old'").unwrap();
        fs::write(dir.join("anomaly.toml"), "enabled = false").unwrap();
        let mut calls = 0;
        let error = snapshot(&[
            ("policy.toml", "mode = 'new'"),
            ("execution_limits.toml", "max_concurrent = 2"),
        ])
        .restore_with(|path, bytes| {
            calls += 1;
            if calls == 2 {
                bail!("injected disk failure");
            }
            write_private_bytes(path, bytes)
        })
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("previous configuration restored"));
        assert_eq!(
            fs::read_to_string(dir.join("policy.toml")).unwrap(),
            "mode = 'old'"
        );
        assert_eq!(
            fs::read_to_string(dir.join("anomaly.toml")).unwrap(),
            "enabled = false"
        );
        assert!(!dir.join("execution_limits.toml").exists());
    }

    #[test]
    fn wrong_password_tampering_and_plaintext_are_rejected() {
        let input = br#"{"version":1,"exportedAt":"2026-10-23T00:00:00Z","files":[{"path":"policy.toml","bytes":3,"content":"x=1"}]}"#;
        let mut encrypted = crate::backup_crypto::encrypt_backup(b"correct", input).unwrap();
        assert_eq!(decode("correct", &encrypted).unwrap().file_count(), 1);
        assert!(decode("wrong", &encrypted).is_err());
        let last = encrypted.len() - 1;
        encrypted[last] ^= 1;
        assert!(decode("correct", &encrypted).is_err());
        assert!(decode("correct", input).is_err());
    }
}
