use crate::store::{config_dir, ensure_config_dir, restrict_file_to_owner};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use subtle::ConstantTimeEq;

pub const MCP_SOURCE_ENV: &str = "AGENT2SSH_SOURCE";
pub const MCP_BINDING_KEY_ENV: &str = "AGENT2SSH_BINDING_KEY";

#[derive(Debug, Default, Serialize, Deserialize)]
struct McpBindingFile {
    #[serde(default)]
    keys: BTreeMap<String, String>,
}

fn binding_path() -> Result<std::path::PathBuf> {
    Ok(config_dir()?.join("mcp_bindings.json"))
}

fn normalize_source(source: &str) -> Result<String> {
    let source = source.trim().to_ascii_lowercase();
    if source.is_empty() {
        return Err(anyhow!("MCP source is empty"));
    }
    if !source
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(anyhow!("MCP source contains unsupported characters"));
    }
    Ok(source)
}

fn load_bindings() -> Result<McpBindingFile> {
    let path = binding_path()?;
    if !path.exists() {
        return Ok(McpBindingFile::default());
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read MCP binding file {}", path.display()))?;
    serde_json::from_str(&raw).context("failed to parse MCP binding file")
}

fn save_bindings(bindings: &McpBindingFile) -> Result<()> {
    ensure_config_dir()?;
    let path = binding_path()?;
    fs::write(&path, serde_json::to_string_pretty(bindings)?)
        .with_context(|| format!("failed to write MCP binding file {}", path.display()))?;
    restrict_file_to_owner(&path)?;
    Ok(())
}

fn new_binding_key() -> Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow!("failed to generate MCP binding key: {e}"))?;
    Ok(hex::encode(bytes))
}

pub fn create_mcp_binding_key(source: &str) -> Result<String> {
    let source = normalize_source(source)?;
    let key = new_binding_key()?;
    let mut bindings = load_bindings()?;
    bindings.keys.insert(source, key.clone());
    save_bindings(&bindings)?;
    Ok(key)
}

pub fn mcp_binding_key_is_valid(source: &str, presented_key: &str) -> Result<bool> {
    let source = normalize_source(source)?;
    let presented_key = presented_key.trim();
    if presented_key.is_empty() {
        return Ok(false);
    }
    let bindings = load_bindings()?;
    let Some(expected_key) = bindings.keys.get(&source) else {
        return Ok(false);
    };
    Ok(expected_key
        .as_bytes()
        .ct_eq(presented_key.as_bytes())
        .into())
}

pub fn verify_mcp_binding_from_env() -> Result<String> {
    let source = std::env::var(MCP_SOURCE_ENV)
        .map_err(|_| anyhow!("{MCP_SOURCE_ENV} is not set; reconfigure the MCP binding"))?;
    let key = std::env::var(MCP_BINDING_KEY_ENV)
        .map_err(|_| anyhow!("{MCP_BINDING_KEY_ENV} is not set; reconfigure the MCP binding"))?;
    if !mcp_binding_key_is_valid(&source, &key)? {
        return Err(anyhow!(
            "MCP binding key is invalid; reconfigure the MCP binding"
        ));
    }
    normalize_source(&source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_config_dir<T>(name: &str, f: impl FnOnce() -> T) -> T {
        let dir = std::env::temp_dir().join(format!(
            "agent2ssh-mcp-binding-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);
        let result = f();
        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = fs::remove_dir_all(dir);
        result
    }

    #[test]
    #[serial_test::serial]
    fn generated_key_verifies_for_source() {
        with_temp_config_dir("valid", || {
            let key = create_mcp_binding_key("codex").unwrap();
            assert!(mcp_binding_key_is_valid("codex", &key).unwrap());
            assert!(!mcp_binding_key_is_valid("cursor", &key).unwrap());
            assert!(!mcp_binding_key_is_valid("codex", "wrong").unwrap());
        });
    }

    #[test]
    #[serial_test::serial]
    fn source_rejects_path_like_values() {
        with_temp_config_dir("source", || {
            assert!(create_mcp_binding_key("../desktop").is_err());
            assert!(create_mcp_binding_key("desktop").is_ok());
        });
    }
}
