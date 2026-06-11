use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::store::config_dir;

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
            path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default()
        ));
        // Handle keys without extension
        let pub_path = if !pub_path.exists() {
            PathBuf::from(format!("{}.pub", path.display()))
        } else {
            pub_path
        };

        let public_key = if pub_path.exists() {
            std::fs::read_to_string(&pub_path).unwrap_or_default().trim().to_string()
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
        }.to_string();

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
    if name.trim().is_empty() {
        return Err(anyhow!("key name is required"));
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(anyhow!("invalid key name"));
    }

    let dir = keys_dir()?;
    let private_path = dir.join(name);
    let public_path = dir.join(format!("{}.pub", name));

    if private_path.exists() {
        return Err(anyhow!("key '{}' already exists", name));
    }

    let comment = comment.unwrap_or_else(|| "agent2ssh");
    let status = std::process::Command::new("ssh-keygen")
        .arg("-t").arg("ed25519")
        .arg("-C").arg(comment)
        .arg("-f").arg(&private_path)
        .arg("-N").arg("") // no passphrase
        .status()
        .map_err(|e| anyhow!("failed to run ssh-keygen: {}", e))?;

    if !status.success() {
        return Err(anyhow!("ssh-keygen failed"));
    }

    let public_key = std::fs::read_to_string(&public_path)
        .unwrap_or_default()
        .trim()
        .to_string();

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
        None => source.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "imported_key".to_string()),
    };

    let dir = keys_dir()?;
    let dest = dir.join(&key_name);

    if dest.exists() {
        return Err(anyhow!("key '{}' already exists in keys directory", key_name));
    }

    std::fs::copy(&source, &dest)?;

    // Set permissions to 600
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&dest, perms)?;
    }

    // Try to copy .pub file too
    let source_pub = PathBuf::from(format!("{}.pub", source.display()));
    let dest_pub = dir.join(format!("{}.pub", key_name));
    if source_pub.exists() {
        let _ = std::fs::copy(&source_pub, &dest_pub);
    }

    let public_key = if dest_pub.exists() {
        std::fs::read_to_string(&dest_pub).unwrap_or_default().trim().to_string()
    } else {
        String::new()
    };

    let key_type = if public_key.starts_with("ssh-ed25519") { "ed25519" }
        else if public_key.starts_with("ssh-rsa") { "rsa" }
        else if public_key.starts_with("ecdsa") { "ecdsa" }
        else { "unknown" }.to_string();

    Ok(SshKeyInfo {
        name: key_name,
        private_path: dest.display().to_string(),
        public_path: dest_pub.display().to_string(),
        public_key,
        key_type,
        created_at: Some(chrono::Utc::now().to_rfc3339()),
    })
}

/// Delete a key pair from the keys directory
pub fn delete_key_core(name: &str) -> Result<()> {
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
        return dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_else(|| path.to_string());
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).display().to_string();
        }
    }
    path.to_string()
}
