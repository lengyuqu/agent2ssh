//! Agent-client integration: register the `agent2ssh-mcp` stdio server into
//! local AI-agent clients (Claude Code, Claude Desktop, Cursor, Codex, …) and
//! install/update/uninstall the bundled Agent Skill (`skills/agent2ssh`).
//!
//! Shared by the CLI (`agent2ssh integrate …`) and the desktop MCP Agents
//! panel; the Tauri layer in `tauri_commands/mcp_agent_config.rs` is a thin
//! wrapper over this module.

use crate::mcp_binding::{create_mcp_binding_key, mcp_binding_key_is_valid, MCP_BINDING_KEY_ENV};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// The Agent Skill shipped with this build, embedded so every binary can
/// install it without a source checkout.
pub const EMBEDDED_SKILL_MD: &str = include_str!("../../skills/agent2ssh/SKILL.md");

const SKILL_DIR_NAME: &str = "agent2ssh";

#[derive(Debug, Clone, Copy)]
pub enum McpConfigFormat {
    Json,
    Toml,
}

#[derive(Debug, Clone)]
pub struct McpClientCandidate {
    pub id: &'static str,
    pub name: &'static str,
    pub source: &'static str,
    pub config_path: PathBuf,
    pub format: McpConfigFormat,
    pub detection_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpAgentConfigStatus {
    pub id: String,
    pub name: String,
    pub source: String,
    pub config_path: String,
    pub detected: bool,
    pub configured: bool,
    pub status: String,
    pub command: Option<String>,
    pub configured_source: Option<String>,
    pub binding_authenticated: bool,
    pub recommended_command: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpAgentConfigureResult {
    pub id: String,
    pub config_path: String,
    pub backup_path: Option<String>,
    pub command: String,
    pub source: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpAgentUninstallResult {
    pub id: String,
    pub config_path: String,
    pub backup_path: Option<String>,
    pub removed: bool,
    pub message: String,
}

fn home_path(relative: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(relative)
}

fn mcp_candidate_paths(legacy_path: &str, platform_path: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(config_dir) = dirs::config_dir() {
        paths.push(config_dir.join(platform_path));
    }
    if let Some(local_dir) = dirs::data_local_dir() {
        paths.push(local_dir.join(platform_path));
    }

    paths.push(home_path(legacy_path));
    paths
}

fn preferred_mcp_path(legacy_path: &str, platform_path: &str) -> PathBuf {
    mcp_candidate_paths(legacy_path, platform_path)
        .into_iter()
        .find(|path| path.exists())
        .or_else(|| {
            dirs::config_dir()
                .map(|config_dir| config_dir.join(platform_path))
                .or_else(|| dirs::data_local_dir().map(|local_dir| local_dir.join(platform_path)))
                .or_else(|| Some(home_path(legacy_path)))
        })
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn mcp_client_candidates() -> Vec<McpClientCandidate> {
    let mut claude_detection_paths =
        mcp_candidate_paths("Library/Application Support/Claude", "Claude");
    let mut cursor_detection_paths = mcp_candidate_paths(".cursor", "Cursor");
    let mut codebuddy_detection_paths =
        mcp_candidate_paths("Library/Application Support/CodeBuddy", "CodeBuddy");
    let mut windsurf_detection_paths = mcp_candidate_paths(".codeium/windsurf", "Codeium/windsurf");
    let mut workbuddy_detection_paths =
        mcp_candidate_paths("Library/Application Support/WorkBuddy", "WorkBuddy");
    let mut qoder_work_detection_paths = mcp_candidate_paths(
        "Library/Application Support/Qoder/SharedClientCache/mcp.json",
        "Qoder/SharedClientCache/mcp.json",
    );
    let mut trae_detection_paths = mcp_candidate_paths("Library/Application Support/Trae", "Trae");
    let mut trae_solo_detection_paths =
        mcp_candidate_paths("Library/Application Support/TRAE SOLO", "TRAE SOLO");

    if cfg!(target_os = "macos") {
        claude_detection_paths.push(PathBuf::from("/Applications/Claude.app"));
        cursor_detection_paths.push(PathBuf::from("/Applications/Cursor.app"));
        codebuddy_detection_paths.push(PathBuf::from("/Applications/CodeBuddy.app"));
        windsurf_detection_paths.push(PathBuf::from("/Applications/Windsurf.app"));
        workbuddy_detection_paths.push(PathBuf::from("/Applications/WorkBuddy.app"));
        qoder_work_detection_paths.push(PathBuf::from("/Applications/QoderWork.app"));
        qoder_work_detection_paths.push(PathBuf::from("/Applications/Qoder.app"));
        trae_detection_paths.push(PathBuf::from("/Applications/Trae.app"));
        trae_solo_detection_paths.push(PathBuf::from("/Applications/TRAE SOLO.app"));
    }

    vec![
        McpClientCandidate {
            id: "claude_code",
            name: "Claude Code",
            source: "claude_code",
            config_path: home_path(".claude.json"),
            format: McpConfigFormat::Json,
            detection_paths: vec![home_path(".claude"), home_path(".claude.json")],
        },
        McpClientCandidate {
            id: "codex",
            name: "Codex",
            source: "codex",
            config_path: home_path(".codex/config.toml"),
            format: McpConfigFormat::Toml,
            detection_paths: vec![home_path(".codex"), home_path(".codex/config.toml")],
        },
        McpClientCandidate {
            id: "gemini_cli",
            name: "Gemini CLI",
            source: "gemini_cli",
            config_path: home_path(".gemini/settings.json"),
            format: McpConfigFormat::Json,
            detection_paths: vec![home_path(".gemini"), home_path(".gemini/settings.json")],
        },
        McpClientCandidate {
            id: "claude_desktop",
            name: "Claude Desktop",
            source: "claude_desktop",
            config_path: preferred_mcp_path(
                "Library/Application Support/Claude/claude_desktop_config.json",
                "Claude/claude_desktop_config.json",
            ),
            format: McpConfigFormat::Json,
            detection_paths: claude_detection_paths,
        },
        McpClientCandidate {
            id: "cursor",
            name: "Cursor",
            source: "cursor",
            config_path: preferred_mcp_path(".cursor/mcp.json", "Cursor/mcp.json"),
            format: McpConfigFormat::Json,
            detection_paths: cursor_detection_paths,
        },
        McpClientCandidate {
            id: "codebuddy",
            name: "CodeBuddy",
            source: "codebuddy",
            config_path: preferred_mcp_path(
                "Library/Application Support/CodeBuddy/mcp.json",
                "CodeBuddy/mcp.json",
            ),
            format: McpConfigFormat::Json,
            detection_paths: {
                codebuddy_detection_paths
                    .into_iter()
                    .chain([home_path(".codebuddy")])
                    .collect()
            },
        },
        McpClientCandidate {
            id: "windsurf",
            name: "Windsurf",
            source: "windsurf",
            config_path: preferred_mcp_path(
                ".codeium/windsurf/mcp_config.json",
                "Codeium/windsurf/mcp_config.json",
            ),
            format: McpConfigFormat::Json,
            detection_paths: windsurf_detection_paths,
        },
        McpClientCandidate {
            id: "workbuddy",
            name: "WorkBuddy",
            source: "workbuddy",
            config_path: preferred_mcp_path(".workbuddy/mcp.json", "WorkBuddy/mcp.json"),
            format: McpConfigFormat::Json,
            detection_paths: {
                let mut paths = workbuddy_detection_paths;
                paths.push(home_path(".workbuddy"));
                paths.push(home_path(
                    "Library/Application Support/@genie/workbuddy-desktop",
                ));
                paths
            },
        },
        McpClientCandidate {
            id: "qoder_work",
            name: "Qoder Work",
            source: "qoder_work",
            config_path: preferred_mcp_path(
                "Library/Application Support/Qoder/SharedClientCache/mcp.json",
                "Qoder/SharedClientCache/mcp.json",
            ),
            format: McpConfigFormat::Json,
            detection_paths: {
                let mut paths = qoder_work_detection_paths;
                paths.push(home_path("Library/Application Support/QoderWork"));
                paths.push(home_path("Library/Application Support/Qoder"));
                paths.push(home_path(".qoderwork"));
                paths.push(home_path(".qoder"));
                paths
            },
        },
        McpClientCandidate {
            id: "trae",
            name: "Trae",
            source: "trae",
            config_path: preferred_mcp_path(
                "Library/Application Support/Trae/User/mcp.json",
                "Trae/User/mcp.json",
            ),
            format: McpConfigFormat::Json,
            detection_paths: {
                trae_detection_paths
                    .into_iter()
                    .chain([home_path(".trae"), home_path(".trae-cn")])
                    .collect()
            },
        },
        McpClientCandidate {
            id: "trae_solo",
            name: "Trae Solo",
            source: "trae_solo",
            config_path: preferred_mcp_path(
                "Library/Application Support/TRAE SOLO/User/mcp.json",
                "TRAE SOLO/User/mcp.json",
            ),
            format: McpConfigFormat::Json,
            detection_paths: {
                trae_solo_detection_paths
                    .into_iter()
                    .chain([home_path(".trae"), home_path(".trae-cn")])
                    .collect()
            },
        },
    ]
}

fn find_candidate(client_id: &str) -> Result<McpClientCandidate> {
    mcp_client_candidates()
        .into_iter()
        .find(|candidate| candidate.id == client_id)
        .ok_or_else(|| {
            let known = mcp_client_candidates()
                .iter()
                .map(|c| c.id)
                .collect::<Vec<_>>()
                .join(", ");
            anyhow!("unknown MCP client '{client_id}' (known: {known})")
        })
}

fn resolve_path_command(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

/// Prefer the `agent2ssh-mcp` binary sitting next to the running executable
/// (bundled desktop / release layout), then fall back to `PATH`.
pub fn resolve_mcp_binary_path() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve current executable directory"))?;
    let candidate = dir.join(format!("agent2ssh-mcp{}", std::env::consts::EXE_SUFFIX));
    if candidate.exists() {
        return Ok(candidate);
    }
    if let Some(path) = resolve_path_command("agent2ssh-mcp") {
        return Ok(path);
    }
    Err(anyhow!("agent2ssh-mcp not found near {}", exe.display()))
}

fn client_detected(candidate: &McpClientCandidate) -> bool {
    candidate.config_path.exists() || candidate.detection_paths.iter().any(|path| path.exists())
}

type ConfiguredEntry = (bool, Option<String>, Option<String>, Option<String>);

fn configured_json(path: &Path) -> Result<ConfiguredEntry> {
    if !path.exists() {
        return Ok((false, None, None, None));
    }
    let raw = std::fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    let agent = value
        .get("mcpServers")
        .and_then(|servers| servers.get("agent2ssh"));
    let command = agent
        .and_then(|server| server.get("command"))
        .and_then(|command| command.as_str())
        .map(ToString::to_string);
    let source = agent
        .and_then(|server| server.get("env"))
        .and_then(|env| env.get("AGENT2SSH_SOURCE"))
        .and_then(|source| source.as_str())
        .map(ToString::to_string);
    let binding_key = agent
        .and_then(|server| server.get("env"))
        .and_then(|env| env.get(MCP_BINDING_KEY_ENV))
        .and_then(|key| key.as_str())
        .map(ToString::to_string);
    Ok((command.is_some(), command, source, binding_key))
}

fn configured_toml(path: &Path) -> Result<ConfiguredEntry> {
    if !path.exists() {
        return Ok((false, None, None, None));
    }
    let raw = std::fs::read_to_string(path)?;
    let value: toml::Value = toml::from_str(&raw)?;
    let agent = value
        .get("mcp_servers")
        .and_then(|servers| servers.get("agent2ssh"));
    let command = agent
        .and_then(|server| server.get("command"))
        .and_then(|command| command.as_str())
        .map(ToString::to_string);
    let source = agent
        .and_then(|server| server.get("env"))
        .and_then(|env| env.get("AGENT2SSH_SOURCE"))
        .and_then(|source| source.as_str())
        .map(ToString::to_string);
    let binding_key = agent
        .and_then(|server| server.get("env"))
        .and_then(|env| env.get(MCP_BINDING_KEY_ENV))
        .and_then(|key| key.as_str())
        .map(ToString::to_string);
    Ok((command.is_some(), command, source, binding_key))
}

fn mcp_configured(candidate: &McpClientCandidate) -> Result<ConfiguredEntry> {
    match candidate.format {
        McpConfigFormat::Json => configured_json(&candidate.config_path),
        McpConfigFormat::Toml => configured_toml(&candidate.config_path),
    }
}

fn backup_config(path: &Path) -> Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let stamp = Utc::now().format("%Y%m%d%H%M%S");
    let backup = path.with_extension(format!(
        "{}.bak-{stamp}",
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("config")
    ));
    std::fs::copy(path, &backup)?;
    Ok(Some(backup))
}

fn configure_json(path: &Path, command: &str, source: &str, binding_key: &str) -> Result<()> {
    let mut value = if path.exists() {
        let raw = std::fs::read_to_string(path)?;
        serde_json::from_str::<serde_json::Value>(&raw)?
    } else {
        serde_json::json!({})
    };
    let root = value
        .as_object_mut()
        .ok_or_else(|| anyhow!("config root must be a JSON object"))?;
    let servers = root
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    let servers = servers
        .as_object_mut()
        .ok_or_else(|| anyhow!("mcpServers must be a JSON object"))?;
    let server = servers
        .entry("agent2ssh")
        .or_insert_with(|| serde_json::json!({}));
    let server = server
        .as_object_mut()
        .ok_or_else(|| anyhow!("mcpServers.agent2ssh must be a JSON object"))?;
    server.insert("command".into(), serde_json::json!(command));
    server.insert("args".into(), serde_json::json!([]));
    let env = server.entry("env").or_insert_with(|| serde_json::json!({}));
    let env = env
        .as_object_mut()
        .ok_or_else(|| anyhow!("mcpServers.agent2ssh.env must be a JSON object"))?;
    env.insert("AGENT2SSH_SOURCE".into(), serde_json::json!(source));
    env.insert(MCP_BINDING_KEY_ENV.into(), serde_json::json!(binding_key));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(&value)?;
    std::fs::write(path, format!("{raw}\n"))?;
    Ok(())
}

fn uninstall_json(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let raw = std::fs::read_to_string(path)?;
    let mut value: serde_json::Value = serde_json::from_str(&raw)?;
    let root = value
        .as_object_mut()
        .ok_or_else(|| anyhow!("config root must be a JSON object"))?;
    let Some(servers) = root.get_mut("mcpServers") else {
        return Ok(false);
    };
    let servers = servers
        .as_object_mut()
        .ok_or_else(|| anyhow!("mcpServers must be a JSON object"))?;
    let removed = servers.remove("agent2ssh").is_some();
    if removed {
        let raw = serde_json::to_string_pretty(&value)?;
        std::fs::write(path, format!("{raw}\n"))?;
    }
    Ok(removed)
}

fn table_mut<'a>(
    table: &'a mut toml::map::Map<String, toml::Value>,
    key: &str,
) -> Result<&'a mut toml::map::Map<String, toml::Value>> {
    let value = table
        .entry(key.to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    value
        .as_table_mut()
        .ok_or_else(|| anyhow!("{key} must be a TOML table"))
}

fn configure_toml(path: &Path, command: &str, source: &str, binding_key: &str) -> Result<()> {
    let mut value = if path.exists() {
        let raw = std::fs::read_to_string(path)?;
        toml::from_str::<toml::Value>(&raw)?
    } else {
        toml::Value::Table(toml::map::Map::new())
    };
    let root = value
        .as_table_mut()
        .ok_or_else(|| anyhow!("config root must be a TOML table"))?;
    let servers = table_mut(root, "mcp_servers")?;
    let agent = table_mut(servers, "agent2ssh")?;
    agent.insert("command".into(), toml::Value::String(command.to_string()));
    agent.insert("args".into(), toml::Value::Array(Vec::new()));
    let env = table_mut(agent, "env")?;
    env.insert(
        "AGENT2SSH_SOURCE".into(),
        toml::Value::String(source.to_string()),
    );
    env.insert(
        MCP_BINDING_KEY_ENV.into(),
        toml::Value::String(binding_key.to_string()),
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = toml::to_string_pretty(&value)?;
    std::fs::write(path, raw)?;
    Ok(())
}

fn uninstall_toml(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let raw = std::fs::read_to_string(path)?;
    let mut value: toml::Value = toml::from_str(&raw)?;
    let root = value
        .as_table_mut()
        .ok_or_else(|| anyhow!("config root must be a TOML table"))?;
    let Some(servers) = root.get_mut("mcp_servers") else {
        return Ok(false);
    };
    let servers = servers
        .as_table_mut()
        .ok_or_else(|| anyhow!("mcp_servers must be a TOML table"))?;
    let removed = servers.remove("agent2ssh").is_some();
    if removed {
        let raw = toml::to_string_pretty(&value)?;
        std::fs::write(path, raw)?;
    }
    Ok(removed)
}

pub fn list_mcp_client_configs() -> Result<Vec<McpAgentConfigStatus>> {
    let command = resolve_mcp_binary_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "agent2ssh-mcp".to_string());
    Ok(mcp_client_candidates()
        .into_iter()
        .map(|candidate| {
            let detected = client_detected(&candidate);
            let (configured, existing_command, configured_source, binding_authenticated, status) =
                match mcp_configured(&candidate) {
                    Ok((has_command, command, source, binding_key)) => {
                        let binding_authenticated =
                            match (source.as_deref(), binding_key.as_deref()) {
                                (Some(source), Some(key)) => {
                                    mcp_binding_key_is_valid(source, key).unwrap_or(false)
                                }
                                _ => false,
                            };
                        let configured = has_command && binding_authenticated;
                        let status = if configured {
                            "configured"
                        } else if has_command {
                            "needs_rebind"
                        } else if detected {
                            "detected"
                        } else {
                            "not_detected"
                        };
                        (
                            configured,
                            command,
                            source,
                            binding_authenticated,
                            status.to_string(),
                        )
                    }
                    Err(error) => (false, None, None, false, format!("invalid_config: {error}")),
                };
            McpAgentConfigStatus {
                id: candidate.id.to_string(),
                name: candidate.name.to_string(),
                source: candidate.source.to_string(),
                config_path: candidate.config_path.display().to_string(),
                detected,
                configured,
                status,
                command: existing_command,
                configured_source,
                binding_authenticated,
                recommended_command: command.clone(),
            }
        })
        .collect())
}

pub fn configure_mcp_client(client_id: &str) -> Result<McpAgentConfigureResult> {
    let candidate = find_candidate(client_id)?;
    let command = resolve_mcp_binary_path()?.display().to_string();
    let binding_key = create_mcp_binding_key(candidate.source)?;
    let backup = backup_config(&candidate.config_path)?;
    match candidate.format {
        McpConfigFormat::Json => configure_json(
            &candidate.config_path,
            &command,
            candidate.source,
            &binding_key,
        )?,
        McpConfigFormat::Toml => configure_toml(
            &candidate.config_path,
            &command,
            candidate.source,
            &binding_key,
        )?,
    }
    Ok(McpAgentConfigureResult {
        id: candidate.id.to_string(),
        config_path: candidate.config_path.display().to_string(),
        backup_path: backup.map(|path| path.display().to_string()),
        command,
        source: candidate.source.to_string(),
        message: format!(
            "{} MCP config updated with a bound source '{}'. Restart the agent client to load agent2ssh.",
            candidate.name, candidate.source
        ),
    })
}

pub fn uninstall_mcp_client(client_id: &str) -> Result<McpAgentUninstallResult> {
    let candidate = find_candidate(client_id)?;
    let backup = backup_config(&candidate.config_path)?;
    let removed = match candidate.format {
        McpConfigFormat::Json => uninstall_json(&candidate.config_path)?,
        McpConfigFormat::Toml => uninstall_toml(&candidate.config_path)?,
    };
    let message = if removed {
        format!(
            "{} MCP binding removed. Restart the agent client to release the old agent2ssh process.",
            candidate.name
        )
    } else {
        format!("{} MCP binding was not present.", candidate.name)
    };
    Ok(McpAgentUninstallResult {
        id: candidate.id.to_string(),
        config_path: candidate.config_path.display().to_string(),
        backup_path: backup.map(|path| path.display().to_string()),
        removed,
        message,
    })
}

// ── Agent Skill install / update / uninstall ────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct AgentSkillStatus {
    /// Directory the skill installs into (e.g. `~/.claude/skills/agent2ssh`).
    pub dir: String,
    /// Full path of the installed `SKILL.md` (whether or not it exists yet).
    pub path: String,
    pub installed: bool,
    pub installed_version: Option<String>,
    pub available_version: Option<String>,
    pub update_available: bool,
}

/// Parse the `version:` field out of a SKILL.md YAML frontmatter block.
fn parse_skill_version(content: &str) -> Option<String> {
    let rest = content.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    for line in rest[..end].lines() {
        if let Some(value) = line.strip_prefix("version:") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

pub fn embedded_skill_version() -> Option<String> {
    parse_skill_version(EMBEDDED_SKILL_MD)
}

/// Default install target: the Claude Code user skills directory.
pub fn default_skill_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("unable to locate home directory"))?;
    Ok(home.join(".claude").join("skills").join(SKILL_DIR_NAME))
}

pub fn agent_skill_status_at(dir: &Path) -> AgentSkillStatus {
    let path = dir.join("SKILL.md");
    let installed_version = std::fs::read_to_string(&path)
        .ok()
        .and_then(|content| parse_skill_version(&content));
    let installed = path.exists();
    let available_version = embedded_skill_version();
    let update_available =
        installed && available_version.is_some() && installed_version != available_version;
    AgentSkillStatus {
        dir: dir.display().to_string(),
        path: path.display().to_string(),
        installed,
        installed_version,
        available_version,
        update_available,
    }
}

pub fn agent_skill_status() -> Result<AgentSkillStatus> {
    Ok(agent_skill_status_at(&default_skill_dir()?))
}

/// Install (or update — same operation) the embedded skill into `dir`.
pub fn install_agent_skill_at(dir: &Path) -> Result<AgentSkillStatus> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("failed to create skill directory {}", dir.display()))?;
    let path = dir.join("SKILL.md");
    std::fs::write(&path, EMBEDDED_SKILL_MD)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(agent_skill_status_at(dir))
}

pub fn install_agent_skill() -> Result<AgentSkillStatus> {
    install_agent_skill_at(&default_skill_dir()?)
}

/// Remove the installed skill. Only deletes `SKILL.md` plus the containing
/// directory when the directory is named after the skill and is empty
/// afterwards — never removes anything else.
pub fn uninstall_agent_skill_at(dir: &Path) -> Result<bool> {
    let path = dir.join("SKILL.md");
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()))?;
    if dir.file_name().and_then(|n| n.to_str()) == Some(SKILL_DIR_NAME) {
        let _ = std::fs::remove_dir(dir); // fails (and is ignored) when non-empty
    }
    Ok(true)
}

pub fn uninstall_agent_skill() -> Result<bool> {
    uninstall_agent_skill_at(&default_skill_dir()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("a2s-integrate-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn embedded_skill_has_frontmatter_version() {
        let version = embedded_skill_version().expect("SKILL.md frontmatter must carry a version");
        assert!(!version.is_empty());
        let normalized = EMBEDDED_SKILL_MD.replace("\r\n", "\n");
        assert!(normalized.starts_with("---\n"));
        assert!(EMBEDDED_SKILL_MD.contains("name: agent2ssh"));
        assert!(EMBEDDED_SKILL_MD.contains("description:"));
    }

    #[test]
    fn parse_skill_version_handles_missing_frontmatter() {
        assert_eq!(parse_skill_version("# no frontmatter"), None);
        assert_eq!(parse_skill_version("---\nname: x\n---\nbody"), None);
        assert_eq!(
            parse_skill_version("---\nname: x\nversion: 1.2.3\n---\nbody"),
            Some("1.2.3".to_string())
        );
    }

    #[test]
    fn skill_install_status_update_uninstall_round_trip() {
        let base = temp_dir("skill");
        let dir = base.join(SKILL_DIR_NAME);

        let status = agent_skill_status_at(&dir);
        assert!(!status.installed);
        assert!(!status.update_available);

        let status = install_agent_skill_at(&dir).unwrap();
        assert!(status.installed);
        assert_eq!(status.installed_version, status.available_version);
        assert!(!status.update_available);

        // Simulate an older installed copy → update_available flips on.
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: agent2ssh\nversion: 0.0.1\n---\nold",
        )
        .unwrap();
        let status = agent_skill_status_at(&dir);
        assert!(status.installed);
        assert!(status.update_available);

        // Install-over acts as update.
        let status = install_agent_skill_at(&dir).unwrap();
        assert!(!status.update_available);

        assert!(uninstall_agent_skill_at(&dir).unwrap());
        assert!(!dir.join("SKILL.md").exists());
        assert!(!dir.exists(), "empty skill dir should be removed");
        assert!(!uninstall_agent_skill_at(&dir).unwrap());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn uninstall_keeps_non_empty_skill_dir() {
        let base = temp_dir("skill-keep");
        let dir = base.join(SKILL_DIR_NAME);
        install_agent_skill_at(&dir).unwrap();
        std::fs::write(dir.join("user-notes.md"), "keep me").unwrap();

        assert!(uninstall_agent_skill_at(&dir).unwrap());
        assert!(dir.exists());
        assert!(dir.join("user-notes.md").exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn configure_json_merges_and_preserves_foreign_keys() {
        let base = temp_dir("json");
        let path = base.join("config.json");
        std::fs::write(
            &path,
            r#"{"theme":"dark","mcpServers":{"other":{"command":"other-bin"}}}"#,
        )
        .unwrap();

        configure_json(&path, "/usr/local/bin/agent2ssh-mcp", "cursor", "key123").unwrap();

        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["theme"], "dark");
        assert_eq!(value["mcpServers"]["other"]["command"], "other-bin");
        assert_eq!(
            value["mcpServers"]["agent2ssh"]["command"],
            "/usr/local/bin/agent2ssh-mcp"
        );
        assert_eq!(
            value["mcpServers"]["agent2ssh"]["env"]["AGENT2SSH_SOURCE"],
            "cursor"
        );

        assert!(uninstall_json(&path).unwrap());
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(value["mcpServers"].get("agent2ssh").is_none());
        assert_eq!(value["mcpServers"]["other"]["command"], "other-bin");
        assert_eq!(value["theme"], "dark");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn configure_toml_merges_and_preserves_foreign_keys() {
        let base = temp_dir("toml");
        let path = base.join("config.toml");
        std::fs::write(
            &path,
            "model = \"gpt\"\n\n[mcp_servers.other]\ncommand = \"other-bin\"\n",
        )
        .unwrap();

        configure_toml(&path, "agent2ssh-mcp", "codex", "key456").unwrap();

        let value: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["model"].as_str(), Some("gpt"));
        assert_eq!(
            value["mcp_servers"]["other"]["command"].as_str(),
            Some("other-bin")
        );
        assert_eq!(
            value["mcp_servers"]["agent2ssh"]["env"]["AGENT2SSH_SOURCE"].as_str(),
            Some("codex")
        );

        assert!(uninstall_toml(&path).unwrap());
        let value: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(value["mcp_servers"].get("agent2ssh").is_none());
        assert_eq!(value["model"].as_str(), Some("gpt"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn candidates_include_claude_code_and_gemini_cli() {
        let candidates = mcp_client_candidates();
        assert!(candidates.iter().any(|c| c.id == "claude_code"));
        assert!(candidates.iter().any(|c| c.id == "gemini_cli"));
        // Every id is unique.
        let mut ids: Vec<_> = candidates.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), candidates.len());
    }
}
