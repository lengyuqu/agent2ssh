use chrono::Utc;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
enum McpConfigFormat {
    Json,
    Toml,
}

#[derive(Debug, Clone)]
struct McpAgentCandidate {
    id: &'static str,
    name: &'static str,
    source: &'static str,
    config_path: PathBuf,
    format: McpConfigFormat,
    detection_paths: Vec<PathBuf>,
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

fn mcp_agent_candidates() -> Vec<McpAgentCandidate> {
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
        McpAgentCandidate {
            id: "codex",
            name: "Codex",
            source: "codex",
            config_path: home_path(".codex/config.toml"),
            format: McpConfigFormat::Toml,
            detection_paths: vec![home_path(".codex"), home_path(".codex/config.toml")],
        },
        McpAgentCandidate {
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
        McpAgentCandidate {
            id: "cursor",
            name: "Cursor",
            source: "cursor",
            config_path: preferred_mcp_path(".cursor/mcp.json", "Cursor/mcp.json"),
            format: McpConfigFormat::Json,
            detection_paths: cursor_detection_paths,
        },
        McpAgentCandidate {
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
        McpAgentCandidate {
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
        McpAgentCandidate {
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
        McpAgentCandidate {
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
        McpAgentCandidate {
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
        McpAgentCandidate {
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

fn resolve_path_command(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn bundled_mcp_binary_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe
        .parent()
        .ok_or_else(|| "failed to resolve current executable directory".to_string())?;
    let candidate = dir.join(format!("agent2ssh-mcp{}", std::env::consts::EXE_SUFFIX));
    if candidate.exists() {
        return Ok(candidate);
    }
    if let Some(path) = resolve_path_command("agent2ssh-mcp") {
        return Ok(path);
    }
    Err(format!("agent2ssh-mcp not found near {}", exe.display()))
}

fn agent_detected(candidate: &McpAgentCandidate) -> bool {
    candidate.config_path.exists() || candidate.detection_paths.iter().any(|path| path.exists())
}

fn configured_json(path: &Path) -> Result<(bool, Option<String>, Option<String>), String> {
    if !path.exists() {
        return Ok((false, None, None));
    }
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
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
    Ok((command.is_some(), command, source))
}

fn configured_toml(path: &Path) -> Result<(bool, Option<String>, Option<String>), String> {
    if !path.exists() {
        return Ok((false, None, None));
    }
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let value: toml::Value = toml::from_str(&raw).map_err(|e| e.to_string())?;
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
    Ok((command.is_some(), command, source))
}

fn mcp_configured(
    candidate: &McpAgentCandidate,
) -> Result<(bool, Option<String>, Option<String>), String> {
    match candidate.format {
        McpConfigFormat::Json => configured_json(&candidate.config_path),
        McpConfigFormat::Toml => configured_toml(&candidate.config_path),
    }
}

fn backup_config(path: &Path) -> Result<Option<PathBuf>, String> {
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
    std::fs::copy(path, &backup).map_err(|e| e.to_string())?;
    Ok(Some(backup))
}

fn configure_json(path: &Path, command: &str, source: &str) -> Result<(), String> {
    let mut value = if path.exists() {
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str::<serde_json::Value>(&raw).map_err(|e| e.to_string())?
    } else {
        serde_json::json!({})
    };
    let Some(root) = value.as_object_mut() else {
        return Err("config root must be a JSON object".into());
    };
    let servers = root
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    let Some(servers) = servers.as_object_mut() else {
        return Err("mcpServers must be a JSON object".into());
    };
    let server = servers
        .entry("agent2ssh")
        .or_insert_with(|| serde_json::json!({}));
    let Some(server) = server.as_object_mut() else {
        return Err("mcpServers.agent2ssh must be a JSON object".into());
    };
    server.insert("command".into(), serde_json::json!(command));
    server.insert("args".into(), serde_json::json!([]));
    let env = server.entry("env").or_insert_with(|| serde_json::json!({}));
    let Some(env) = env.as_object_mut() else {
        return Err("mcpServers.agent2ssh.env must be a JSON object".into());
    };
    env.insert("AGENT2SSH_SOURCE".into(), serde_json::json!(source));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    std::fs::write(path, format!("{raw}\n")).map_err(|e| e.to_string())
}

fn uninstall_json(path: &Path) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let Some(root) = value.as_object_mut() else {
        return Err("config root must be a JSON object".into());
    };
    let Some(servers) = root.get_mut("mcpServers") else {
        return Ok(false);
    };
    let Some(servers) = servers.as_object_mut() else {
        return Err("mcpServers must be a JSON object".into());
    };
    let removed = servers.remove("agent2ssh").is_some();
    if removed {
        let raw = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
        std::fs::write(path, format!("{raw}\n")).map_err(|e| e.to_string())?;
    }
    Ok(removed)
}

fn table_mut<'a>(
    table: &'a mut toml::map::Map<String, toml::Value>,
    key: &str,
) -> Result<&'a mut toml::map::Map<String, toml::Value>, String> {
    let value = table
        .entry(key.to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    value
        .as_table_mut()
        .ok_or_else(|| format!("{key} must be a TOML table"))
}

fn configure_toml(path: &Path, command: &str, source: &str) -> Result<(), String> {
    let mut value = if path.exists() {
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        toml::from_str::<toml::Value>(&raw).map_err(|e| e.to_string())?
    } else {
        toml::Value::Table(toml::map::Map::new())
    };
    let Some(root) = value.as_table_mut() else {
        return Err("config root must be a TOML table".into());
    };
    let servers = table_mut(root, "mcp_servers")?;
    let agent = table_mut(servers, "agent2ssh")?;
    agent.insert("command".into(), toml::Value::String(command.to_string()));
    agent.insert("args".into(), toml::Value::Array(Vec::new()));
    let env = table_mut(agent, "env")?;
    env.insert(
        "AGENT2SSH_SOURCE".into(),
        toml::Value::String(source.to_string()),
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = toml::to_string_pretty(&value).map_err(|e| e.to_string())?;
    std::fs::write(path, raw).map_err(|e| e.to_string())
}

fn uninstall_toml(path: &Path) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut value: toml::Value = toml::from_str(&raw).map_err(|e| e.to_string())?;
    let Some(root) = value.as_table_mut() else {
        return Err("config root must be a TOML table".into());
    };
    let Some(servers) = root.get_mut("mcp_servers") else {
        return Ok(false);
    };
    let Some(servers) = servers.as_table_mut() else {
        return Err("mcp_servers must be a TOML table".into());
    };
    let removed = servers.remove("agent2ssh").is_some();
    if removed {
        let raw = toml::to_string_pretty(&value).map_err(|e| e.to_string())?;
        std::fs::write(path, raw).map_err(|e| e.to_string())?;
    }
    Ok(removed)
}

#[tauri::command]
pub fn list_mcp_agent_configs() -> Result<Vec<McpAgentConfigStatus>, String> {
    let command = bundled_mcp_binary_path()?.display().to_string();
    Ok(mcp_agent_candidates()
        .into_iter()
        .map(|candidate| {
            let detected = agent_detected(&candidate);
            let (configured, existing_command, configured_source, status) =
                match mcp_configured(&candidate) {
                    Ok((configured, command, source)) => {
                        let status = if configured {
                            "configured"
                        } else if detected {
                            "detected"
                        } else {
                            "not_detected"
                        };
                        (configured, command, source, status.to_string())
                    }
                    Err(error) => (false, None, None, format!("invalid_config: {error}")),
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
                recommended_command: command.clone(),
            }
        })
        .collect())
}

#[tauri::command]
pub fn configure_mcp_agent(agent_id: String) -> Result<McpAgentConfigureResult, String> {
    let candidate = mcp_agent_candidates()
        .into_iter()
        .find(|candidate| candidate.id == agent_id)
        .ok_or_else(|| format!("unknown MCP agent: {agent_id}"))?;
    let command = bundled_mcp_binary_path()?.display().to_string();
    let backup = backup_config(&candidate.config_path)?;
    match candidate.format {
        McpConfigFormat::Json => {
            configure_json(&candidate.config_path, &command, candidate.source)?
        }
        McpConfigFormat::Toml => {
            configure_toml(&candidate.config_path, &command, candidate.source)?
        }
    }
    Ok(McpAgentConfigureResult {
        id: candidate.id.to_string(),
        config_path: candidate.config_path.display().to_string(),
        backup_path: backup.map(|path| path.display().to_string()),
        command,
        source: candidate.source.to_string(),
        message: format!(
            "{} MCP config updated with source '{}'. Restart the agent client to load agent2ssh.",
            candidate.name, candidate.source
        ),
    })
}

#[tauri::command]
pub fn uninstall_mcp_agent(agent_id: String) -> Result<McpAgentUninstallResult, String> {
    let candidate = mcp_agent_candidates()
        .into_iter()
        .find(|candidate| candidate.id == agent_id)
        .ok_or_else(|| format!("unknown MCP agent: {agent_id}"))?;
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
