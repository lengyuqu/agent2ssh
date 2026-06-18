use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use ssh_key::{
    private::{Ed25519Keypair, KeypairData},
    LineEnding, PrivateKey,
};
use std::path::PathBuf;

use crate::store::{config_dir, restrict_file_to_owner};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshKeyInfo {
    /// Filename of the private key (e.g. "id_ed25519_work")
    pub name: String,
    /// Full path to the private key file
    pub private_path: String,
    /// Full path to the public key file (name + ".pub")
    pub public_path: String,
    /// The public key content (e.g. "ssh-ed25519 AAAA... user@host")
    #[serde(default)]
    pub public_key: String,
    /// Key type (e.g. "ed25519", "rsa")
    #[serde(default)]
    pub key_type: String,
    /// Creation timestamp
    #[serde(default)]
    pub created_at: Option<String>,
}

fn keys_dir() -> Result<PathBuf> {
    let dir = config_dir()?.join("keys");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn restrict_private_key_permissions(path: impl AsRef<std::path::Path>) -> Result<()> {
    restrict_file_to_owner(path)
}

fn validate_key_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(anyhow!("key name is required"));
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(anyhow!("invalid key name"));
    }
    Ok(())
}

/// List all key pairs in ~/.agent2ssh/keys/
pub fn list_keys_core() -> Result<Vec<SshKeyInfo>> {
    let dir = keys_dir()?;
    let mut keys = Vec::new();

    if !dir.exists() {
        return Ok(keys);
    }

    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip .pub files and hidden files
        if name.ends_with(".pub") || name.starts_with('.') {
            continue;
        }

        let pub_path = path.with_extension(format!(
            "{}.pub",
            path.extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default()
        ));
        // Handle keys without extension
        let pub_path = if !pub_path.exists() {
            PathBuf::from(format!("{}.pub", path.display()))
        } else {
            pub_path
        };

        let public_key = if pub_path.exists() {
            std::fs::read_to_string(&pub_path)
                .unwrap_or_default()
                .trim()
                .to_string()
        } else {
            String::new()
        };

        let key_type = if name.contains("ed25519") {
            "ed25519"
        } else if name.contains("rsa") {
            "rsa"
        } else if name.contains("ecdsa") {
            "ecdsa"
        } else if public_key.starts_with("ssh-ed25519") {
            "ed25519"
        } else if public_key.starts_with("ssh-rsa") {
            "rsa"
        } else {
            "unknown"
        }
        .to_string();

        let created_at = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.created().ok())
            .map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.to_rfc3339()
            });

        keys.push(SshKeyInfo {
            name: name.clone(),
            private_path: path.display().to_string(),
            public_path: pub_path.display().to_string(),
            public_key,
            key_type,
            created_at,
        });
    }

    keys.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(keys)
}

/// Generate a new Ed25519 key pair
pub fn generate_key_core(name: &str, comment: Option<&str>) -> Result<SshKeyInfo> {
    validate_key_name(name)?;

    let dir = keys_dir()?;
    let private_path = dir.join(name);
    let public_path = dir.join(format!("{}.pub", name));

    if private_path.exists() {
        return Err(anyhow!("key '{}' already exists", name));
    }

    let comment = comment.unwrap_or("agent2ssh");
    let mut seed = [0u8; ssh_key::private::Ed25519PrivateKey::BYTE_SIZE];
    getrandom::fill(&mut seed).map_err(|e| anyhow!("failed to read system random source: {e}"))?;
    let keypair = Ed25519Keypair::from_seed(&seed);
    let private_key = PrivateKey::new(KeypairData::from(keypair), comment)?;
    let private_pem = private_key.to_openssh(LineEnding::LF)?;
    let public_key = private_key.public_key().to_openssh()?;

    std::fs::write(&private_path, private_pem.as_bytes())?;
    restrict_private_key_permissions(&private_path)?;
    std::fs::write(&public_path, format!("{public_key}\n"))?;

    Ok(SshKeyInfo {
        name: name.to_string(),
        private_path: private_path.display().to_string(),
        public_path: public_path.display().to_string(),
        public_key,
        key_type: "ed25519".to_string(),
        created_at: Some(chrono::Utc::now().to_rfc3339()),
    })
}

/// Import an existing private key file into the keys directory
pub fn import_key_core(source_path: &str, name: Option<&str>) -> Result<SshKeyInfo> {
    let source = PathBuf::from(expand_tilde(source_path));
    if !source.exists() {
        return Err(anyhow!("source key not found: {}", source.display()));
    }

    let key_name: String = match name {
        Some(n) => n.to_string(),
        None => source
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "imported_key".to_string()),
    };
    validate_key_name(&key_name)?;

    let dir = keys_dir()?;
    let dest = dir.join(&key_name);

    if dest.exists() {
        return Err(anyhow!(
            "key '{}' already exists in keys directory",
            key_name
        ));
    }

    std::fs::copy(&source, &dest)?;
    restrict_private_key_permissions(&dest)?;

    // Try to copy .pub file too
    let source_pub = PathBuf::from(format!("{}.pub", source.display()));
    let dest_pub = dir.join(format!("{}.pub", key_name));
    if source_pub.exists() {
        let _ = std::fs::copy(&source_pub, &dest_pub);
    }

    let public_key = if dest_pub.exists() {
        std::fs::read_to_string(&dest_pub)
            .unwrap_or_default()
            .trim()
            .to_string()
    } else {
        String::new()
    };

    let key_type = if public_key.starts_with("ssh-ed25519") {
        "ed25519"
    } else if public_key.starts_with("ssh-rsa") {
        "rsa"
    } else if public_key.starts_with("ecdsa") {
        "ecdsa"
    } else {
        "unknown"
    }
    .to_string();

    Ok(SshKeyInfo {
        name: key_name,
        private_path: dest.display().to_string(),
        public_path: dest_pub.display().to_string(),
        public_key,
        key_type,
        created_at: Some(chrono::Utc::now().to_rfc3339()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn test_generate_key_core_writes_openssh_ed25519_pair() {
        let original_config_dir = std::env::var("AGENT2SSH_CONFIG_DIR").ok();
        let config_dir =
            std::env::temp_dir().join(format!("agent2ssh-keygen-{}", uuid::Uuid::new_v4()));
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &config_dir);

        let result = generate_key_core("id_ed25519_test", Some("agent2ssh-test")).unwrap();
        let private_raw = std::fs::read_to_string(&result.private_path).unwrap();
        let public_raw = std::fs::read_to_string(&result.public_path).unwrap();

        let private_key = PrivateKey::from_openssh(&private_raw).unwrap();
        assert_eq!(private_key.algorithm(), ssh_key::Algorithm::Ed25519);
        assert_eq!(private_key.comment().as_str().unwrap(), "agent2ssh-test");
        assert!(private_raw.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
        assert!(public_raw.starts_with("ssh-ed25519 "));
        assert_eq!(
            public_raw.trim(),
            private_key.public_key().to_openssh().unwrap()
        );

        let _ = std::fs::remove_dir_all(&config_dir);
        match original_config_dir {
            Some(value) => std::env::set_var("AGENT2SSH_CONFIG_DIR", value),
            None => std::env::remove_var("AGENT2SSH_CONFIG_DIR"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_restrict_private_key_permissions_sets_0600() {
        use std::os::unix::fs::PermissionsExt;

        let path =
            std::env::temp_dir().join(format!("agent2ssh-key-perms-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, "private-key").unwrap();

        restrict_private_key_permissions(&path).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let _ = std::fs::remove_file(&path);
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn test_validate_key_name_rejects_path_traversal() {
        assert!(validate_key_name("../id_ed25519").is_err());
        assert!(validate_key_name("nested/id_ed25519").is_err());
        assert!(validate_key_name("nested\\id_ed25519").is_err());
        assert!(validate_key_name("id_ed25519").is_ok());
    }
}

/// Delete a key pair from the keys directory
pub fn delete_key_core(name: &str) -> Result<()> {
    validate_key_name(name)?;
    let dir = keys_dir()?;
    let private = dir.join(name);
    let public = dir.join(format!("{}.pub", name));

    if !private.exists() {
        return Err(anyhow!("key '{}' not found", name));
    }

    std::fs::remove_file(&private)?;
    if public.exists() {
        let _ = std::fs::remove_file(&public);
    }
    Ok(())
}

fn expand_tilde(path: &str) -> String {
    if path == "~" {
        return dirs::home_dir()
            .map(|h| h.display().to_string())
            .unwrap_or_else(|| path.to_string());
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).display().to_string();
        }
    }
    path.to_string()
}
