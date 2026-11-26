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

/// The Agent Skill's entry file, shipped with this build and embedded so every
/// binary can install it without a source checkout.
pub const EMBEDDED_SKILL_MD: &str = include_str!("../../skills/agent2ssh/SKILL.md");

/// Every file of the bundled skill, as a path relative to the skill directory.
///
/// `SKILL.md` is what an agent loads when the skill triggers, so it holds only
/// what always applies; the detail for each area is a file under `references/`
/// that the agent reads when the task needs it. All of them have to be installed
/// together — `SKILL.md` points at the others — which is why install, update and
/// uninstall go through this table rather than a single file.
///
/// Add a file here when it is added under `skills/agent2ssh/`, and link it from
/// `SKILL.md`: `every_skill_file_is_linked_and_every_link_resolves` fails in
/// both directions.
pub const EMBEDDED_SKILL_FILES: &[(&str, &str)] = &[
    ("SKILL.md", EMBEDDED_SKILL_MD),
    (
        "references/exec.md",
        include_str!("../../skills/agent2ssh/references/exec.md"),
    ),
    (
        "references/hosts-and-credentials.md",
        include_str!("../../skills/agent2ssh/references/hosts-and-credentials.md"),
    ),
    (
        "references/files-sessions-forwards.md",
        include_str!("../../skills/agent2ssh/references/files-sessions-forwards.md"),
    ),
    (
        "references/local-and-resident.md",
        include_str!("../../skills/agent2ssh/references/local-and-resident.md"),
    ),
    (
        "references/config-and-audit.md",
        include_str!("../../skills/agent2ssh/references/config-and-audit.md"),
    ),
    (
        "references/workflows.md",
        include_str!("../../skills/agent2ssh/references/workflows.md"),
    ),
];

/// Directory under the skill directory that holds the reference files.
const SKILL_REFERENCES_DIR: &str = "references";

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
    /// Executable names that count as an install when one of them is on `PATH`.
    pub cli_names: &'static [&'static str],
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

/// Per-user config root: `~/Library/Application Support` on macOS, `%APPDATA%`
/// on Windows, `~/.config` on Linux.
fn user_config_file(parts: &[&str]) -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| home_path(".config"));
    for part in parts {
        path = path.join(part);
    }
    path
}

fn user_data_dir(name: &str) -> PathBuf {
    user_config_file(&[name])
}

/// Install locations that differ by operating system. A hit on any of them, or
/// on a CLI name later, means the client is installed. Home dot-directories
/// (`.claude`, `.trae-cn`, …) are the same shape on every OS and are added by
/// the caller.
fn install_evidence(app_names: &[&str]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if cfg!(target_os = "macos") {
        for name in app_names {
            let bundle = format!("{name}.app");
            paths.push(PathBuf::from("/Applications").join(&bundle));
            paths.push(home_path("Applications").join(bundle));
        }
    } else if cfg!(windows) {
        let mut roots = Vec::new();
        for key in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(value) = std::env::var_os(key) {
                roots.push(PathBuf::from(value));
            }
        }
        if let Some(local) = dirs::data_local_dir() {
            roots.push(local.join("Programs"));
        }
        for root in roots {
            for name in app_names {
                paths.push(root.join(name));
            }
        }
    } else {
        for name in app_names {
            let slug = name.to_ascii_lowercase().replace(' ', "-");
            paths.push(PathBuf::from("/opt").join(name));
            paths.push(PathBuf::from("/usr/bin").join(&slug));
            paths.push(PathBuf::from("/usr/share/applications").join(format!("{slug}.desktop")));
            paths.push(home_path(".local/share/applications").join(format!("{slug}.desktop")));
        }
    }
    paths
}

struct ClientSpec {
    id: &'static str,
    name: &'static str,
    source: &'static str,
    /// MCP files this client reads, most current first. Registration uses the
    /// first one that already exists, and creates the first one otherwise.
    mcp_configs: Vec<PathBuf>,
    mcp_format: McpConfigFormat,
    /// Skill directories this client reads, most current first. Empty when the
    /// client has no skill loader. Same selection rule as `mcp_configs`,
    /// except a directory counts only when it already holds `SKILL.md`.
    skill_dirs: Vec<PathBuf>,
    detection_paths: Vec<PathBuf>,
    cli_names: &'static [&'static str],
}

fn skill_install_dir(dot_dir: &str) -> PathBuf {
    home_path(dot_dir).join("skills").join(SKILL_DIR_NAME)
}

/// First candidate that is a real file, otherwise the first candidate.
///
/// A directory at the same path does not count: writing through it would fail,
/// and it is not the config the client is reading.
fn select_existing_file(candidates: &[PathBuf]) -> PathBuf {
    candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .or_else(|| candidates.first().cloned())
        .unwrap_or_default()
}

/// First skill directory that already holds `SKILL.md`, otherwise the first
/// candidate. An empty skills folder is not an install.
fn select_installed_skill_dir(candidates: &[PathBuf]) -> PathBuf {
    candidates
        .iter()
        .find(|dir| dir.join("SKILL.md").is_file())
        .cloned()
        .or_else(|| candidates.first().cloned())
        .unwrap_or_default()
}

fn unique_paths(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in paths {
        if !out.iter().any(|seen| seen == &path) {
            out.push(path);
        }
    }
    out
}

/// Config-directory locations of one relative path, native directory first.
///
/// `dirs::config_dir` is `%APPDATA%` on Windows, `~/Library/Application Support`
/// on macOS, and `$XDG_CONFIG_HOME` or `~/.config` on Linux. The other roots
/// stay in the list so a file that was created under a different convention
/// (or copied across machines) is updated in place. The native path stays
/// first, which is where a missing file is created.
fn user_config_candidates(parts: &[&str]) -> Vec<PathBuf> {
    let relative = parts
        .iter()
        .fold(PathBuf::new(), |path, part| path.join(part));
    let roaming = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_path("AppData").join("Roaming"));
    unique_paths([
        user_config_file(parts),
        home_path(".config").join(&relative),
        home_path("Library")
            .join("Application Support")
            .join(&relative),
        roaming.join(relative),
    ])
}

/// One place a client keeps a config file, plus the directories that mean
/// this place is the install in use.
struct Layout {
    target: PathBuf,
    markers: Vec<PathBuf>,
}

/// Move the first layout whose marker exists to the front.
///
/// File existence is applied afterwards by [`select_existing_file`]: a layout
/// that is installed but has no file yet becomes the create target, and a
/// file that already exists is still chosen even when it is not that layout.
fn order_layouts(layouts: &[Layout]) -> Vec<PathBuf> {
    let promoted = layouts
        .iter()
        .position(|layout| layout.markers.iter().any(|path| path.exists()));
    match promoted {
        Some(index) => std::iter::once(layouts[index].target.clone())
            .chain(
                layouts
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != index)
                    .map(|(_, layout)| layout.target.clone()),
            )
            .collect(),
        None => layouts.iter().map(|layout| layout.target.clone()).collect(),
    }
}

fn codebuddy_config_dir() -> PathBuf {
    std::env::var_os("CODEBUDDY_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_path(".codebuddy"))
}

/// CodeBuddy reads the first of these that exists and, when none do, creates
/// `.mcp.json`. `mcp.json` and `~/.codebuddy.json` are the older files.
fn codebuddy_mcp_configs(dir: &Path, legacy: PathBuf) -> Vec<PathBuf> {
    vec![dir.join(".mcp.json"), dir.join("mcp.json"), legacy]
}

/// Current Devin Desktop file. macOS and Linux use `~/.config` (or
/// `$XDG_CONFIG_HOME`), not the platform config directory — on macOS that
/// directory is `~/Library/Application Support`, which Devin does not read.
/// Windows uses `%APPDATA%`.
fn devin_mcp_config() -> PathBuf {
    if cfg!(windows) {
        return user_config_file(&["devin", "mcp_config.json"]);
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(xdg).join("devin").join("mcp_config.json");
    }
    home_path(".config/devin/mcp_config.json")
}

fn windsurf_mcp_configs() -> Vec<PathBuf> {
    let legacy = home_path(".codeium/windsurf/mcp_config.json");
    let current = devin_mcp_config();
    let mut windsurf_markers = vec![home_path(".codeium/windsurf")];
    windsurf_markers.extend(install_evidence(&["Windsurf"]));
    let mut devin_markers = Vec::new();
    if let Some(parent) = current.parent() {
        devin_markers.push(parent.to_path_buf());
    }
    devin_markers.extend(install_evidence(&["Devin"]));
    let mut configs = order_layouts(&[
        Layout {
            target: legacy,
            markers: windsurf_markers,
        },
        Layout {
            target: current.clone(),
            markers: devin_markers,
        },
    ]);
    let dot_config = home_path(".config/devin/mcp_config.json");
    if !configs.iter().any(|path| path == &dot_config) {
        configs.push(dot_config);
    }
    configs
}

fn with_apps(mut paths: Vec<PathBuf>, app_names: &[&str]) -> Vec<PathBuf> {
    paths.extend(install_evidence(app_names));
    paths
}

fn client_specs() -> Vec<ClientSpec> {
    let codebuddy_dir = codebuddy_config_dir();
    vec![
        ClientSpec {
            id: "claude_code",
            name: "Claude Code",
            source: "claude_code",
            // User MCP lives in `~/.claude.json` on every OS (`%USERPROFILE%`
            // on Windows). Project `.mcp.json` is per-repo and is not written
            // from here.
            mcp_configs: vec![home_path(".claude.json")],
            mcp_format: McpConfigFormat::Json,
            skill_dirs: vec![skill_install_dir(".claude")],
            detection_paths: vec![home_path(".claude"), home_path(".claude.json")],
            cli_names: &["claude"],
        },
        ClientSpec {
            id: "codex",
            name: "Codex",
            source: "codex",
            mcp_configs: vec![home_path(".codex/config.toml")],
            mcp_format: McpConfigFormat::Toml,
            skill_dirs: vec![skill_install_dir(".codex")],
            detection_paths: vec![home_path(".codex"), home_path(".codex/config.toml")],
            cli_names: &["codex"],
        },
        ClientSpec {
            id: "claude_desktop",
            name: "Claude Desktop",
            source: "claude_desktop",
            mcp_configs: user_config_candidates(&["Claude", "claude_desktop_config.json"]),
            mcp_format: McpConfigFormat::Json,
            skill_dirs: vec![],
            detection_paths: with_apps(vec![user_data_dir("Claude")], &["Claude"]),
            cli_names: &[],
        },
        ClientSpec {
            id: "cursor",
            name: "Cursor",
            source: "cursor",
            mcp_configs: vec![home_path(".cursor/mcp.json")],
            mcp_format: McpConfigFormat::Json,
            skill_dirs: vec![skill_install_dir(".cursor")],
            detection_paths: with_apps(
                vec![home_path(".cursor"), user_data_dir("Cursor")],
                &["Cursor"],
            ),
            cli_names: &["cursor"],
        },
        ClientSpec {
            id: "codebuddy",
            name: "CodeBuddy",
            source: "codebuddy",
            mcp_configs: codebuddy_mcp_configs(&codebuddy_dir, home_path(".codebuddy.json")),
            mcp_format: McpConfigFormat::Json,
            skill_dirs: vec![codebuddy_dir.join("skills").join(SKILL_DIR_NAME)],
            detection_paths: with_apps(
                vec![codebuddy_dir, user_data_dir("CodeBuddy")],
                &["CodeBuddy"],
            ),
            cli_names: &["codebuddy"],
        },
        ClientSpec {
            id: "windsurf",
            name: "Windsurf",
            source: "windsurf",
            // Older Windsurf reads `~/.codeium/windsurf/mcp_config.json`. Devin
            // Desktop (the current app) reads a different file, and which one
            // is created follows the install that is actually present.
            mcp_configs: windsurf_mcp_configs(),
            mcp_format: McpConfigFormat::Json,
            skill_dirs: vec![],
            detection_paths: with_apps(
                vec![
                    home_path(".codeium/windsurf"),
                    home_path(".config/devin"),
                    user_data_dir("Windsurf"),
                    user_data_dir("devin"),
                ],
                &["Windsurf", "Devin"],
            ),
            cli_names: &["windsurf"],
        },
        ClientSpec {
            id: "workbuddy",
            name: "WorkBuddy",
            source: "workbuddy",
            mcp_configs: vec![home_path(".workbuddy/mcp.json")],
            mcp_format: McpConfigFormat::Json,
            skill_dirs: vec![skill_install_dir(".workbuddy")],
            detection_paths: with_apps(
                vec![home_path(".workbuddy"), user_data_dir("WorkBuddy")],
                &["WorkBuddy"],
            ),
            cli_names: &["workbuddy"],
        },
        ClientSpec {
            id: "qoder",
            name: "Qoder",
            source: "qoder",
            // Connectors live in `mcpServers` inside settings.json. A bare
            // `~/.qoder` is not an install: this machine keeps leftover logs there.
            mcp_configs: vec![home_path(".qoder/settings.json")],
            mcp_format: McpConfigFormat::Json,
            skill_dirs: vec![skill_install_dir(".qoder")],
            detection_paths: with_apps(
                vec![
                    home_path(".qoder/settings.json"),
                    user_data_dir("com.qoder.app.stable"),
                ],
                &["Qoder"],
            ),
            cli_names: &["qoder"],
        },
        ClientSpec {
            id: "qoder_cn",
            name: "Qoder CN",
            source: "qoder_cn",
            mcp_configs: vec![home_path(".qoder-cn/settings.json")],
            mcp_format: McpConfigFormat::Json,
            skill_dirs: vec![skill_install_dir(".qoder-cn")],
            detection_paths: with_apps(
                vec![
                    home_path(".qoder-cn"),
                    user_data_dir("com.qodercn.app.stable"),
                ],
                &["Qoder CN"],
            ),
            cli_names: &["qodercn", "qoder-cn"],
        },
        ClientSpec {
            id: "trae",
            name: "Trae",
            source: "trae",
            mcp_configs: user_config_candidates(&["Trae", "User", "mcp.json"]),
            mcp_format: McpConfigFormat::Json,
            skill_dirs: vec![skill_install_dir(".trae")],
            detection_paths: with_apps(vec![user_data_dir("Trae"), home_path(".trae")], &["Trae"]),
            cli_names: &["trae"],
        },
        ClientSpec {
            id: "trae_cn",
            name: "Trae CN",
            source: "trae_cn",
            mcp_configs: user_config_candidates(&["Trae CN", "User", "mcp.json"]),
            mcp_format: McpConfigFormat::Json,
            skill_dirs: vec![skill_install_dir(".trae-cn")],
            detection_paths: with_apps(
                vec![user_data_dir("Trae CN"), home_path(".trae-cn")],
                &["Trae CN", "TRAE CN"],
            ),
            cli_names: &["trae-cn", "traecn"],
        },
        ClientSpec {
            id: "agy",
            name: "Antigravity",
            source: "agy",
            // Current builds read `~/.gemini/config/mcp_config.json`. Older
            // installs still have `~/.gemini/antigravity/mcp_config.json`.
            // The App and the CLI share `config/skills`; the CLI also reads
            // the two later skill directories, which the App ignores.
            mcp_configs: vec![
                home_path(".gemini/config/mcp_config.json"),
                home_path(".gemini/antigravity/mcp_config.json"),
            ],
            mcp_format: McpConfigFormat::Json,
            skill_dirs: vec![
                home_path(".gemini/config/skills").join(SKILL_DIR_NAME),
                home_path(".gemini/antigravity-cli/skills").join(SKILL_DIR_NAME),
                home_path(".gemini/skills").join(SKILL_DIR_NAME),
            ],
            // A bare `~/.gemini` is the old Gemini CLI, which is not this
            // client. Only the Antigravity layouts count, plus the app bundle
            // and the `agy` binary.
            detection_paths: with_apps(
                vec![
                    home_path(".gemini/config"),
                    home_path(".gemini/antigravity"),
                    home_path(".gemini/antigravity-cli"),
                ],
                &["Antigravity"],
            ),
            cli_names: &["agy"],
        },
    ]
}

pub fn mcp_client_candidates() -> Vec<McpClientCandidate> {
    client_specs()
        .into_iter()
        .map(|spec| McpClientCandidate {
            id: spec.id,
            name: spec.name,
            source: spec.source,
            config_path: select_existing_file(&spec.mcp_configs),
            format: spec.mcp_format,
            detection_paths: spec.detection_paths,
            cli_names: spec.cli_names,
        })
        .collect()
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

fn path_or_cli_detected(paths: &[PathBuf], cli_names: &[&str], config_path: Option<&Path>) -> bool {
    paths.iter().any(|path| path.exists())
        || cli_names
            .iter()
            .any(|name| resolve_path_command(name).is_some())
        || config_path.is_some_and(|path| path.exists())
}

fn client_detected(candidate: &McpClientCandidate) -> bool {
    path_or_cli_detected(
        &candidate.detection_paths,
        candidate.cli_names,
        Some(&candidate.config_path),
    )
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
    // An update is due when the version differs, or when a file this build ships
    // is gone — an install from before the skill had reference files, or one
    // someone deleted from, points at files that are not there. Content is not
    // compared: a skill edited by hand with the version left alone is the
    // owner's, and would otherwise ask to be overwritten forever.
    let incomplete = EMBEDDED_SKILL_FILES
        .iter()
        .any(|(relative, _)| !dir.join(relative).exists());
    let update_available = installed
        && available_version.is_some()
        && (installed_version != available_version || incomplete);
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

/// Install (or update — same operation) the embedded skill into `dir`: every
/// file of [`EMBEDDED_SKILL_FILES`], `references/` included.
pub fn install_agent_skill_at(dir: &Path) -> Result<AgentSkillStatus> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("failed to create skill directory {}", dir.display()))?;
    for (relative, content) in EMBEDDED_SKILL_FILES {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::write(&path, content)
            .with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(agent_skill_status_at(dir))
}

pub fn install_agent_skill() -> Result<AgentSkillStatus> {
    install_agent_skill_at(&default_skill_dir()?)
}

/// Remove the installed skill: the files of [`EMBEDDED_SKILL_FILES`] that are
/// there, then `references/` and the skill directory when each is left empty
/// (the skill directory only when it is named after the skill). Never removes a
/// file this build did not install — anything else in the directory, including a
/// file someone added to `references/`, stays.
///
/// Returns whether any file was removed. An install from before the skill had
/// reference files holds only `SKILL.md`, and is removed the same way.
pub fn uninstall_agent_skill_at(dir: &Path) -> Result<bool> {
    let mut removed = false;
    for (relative, _) in EMBEDDED_SKILL_FILES {
        let path = dir.join(relative);
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
            removed = true;
        }
    }
    if !removed {
        return Ok(false);
    }
    // Each fails (and is ignored) when something else is still inside.
    let _ = std::fs::remove_dir(dir.join(SKILL_REFERENCES_DIR));
    if dir.file_name().and_then(|n| n.to_str()) == Some(SKILL_DIR_NAME) {
        let _ = std::fs::remove_dir(dir);
    }
    Ok(true)
}

pub fn uninstall_agent_skill() -> Result<bool> {
    uninstall_agent_skill_at(&default_skill_dir()?)
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillHarnessStatus {
    pub id: String,
    pub name: String,
    pub detected: bool,
    pub dir: String,
    pub path: String,
    pub installed: bool,
    pub installed_version: Option<String>,
    pub available_version: Option<String>,
    pub update_available: bool,
}

struct SkillHarness {
    id: &'static str,
    name: &'static str,
    dir: PathBuf,
    detection_paths: Vec<PathBuf>,
    cli_names: &'static [&'static str],
}

fn skill_harnesses() -> Vec<SkillHarness> {
    client_specs()
        .into_iter()
        .filter_map(|spec| {
            if spec.skill_dirs.is_empty() {
                return None;
            }
            Some(SkillHarness {
                id: spec.id,
                name: spec.name,
                dir: select_installed_skill_dir(&spec.skill_dirs),
                detection_paths: spec.detection_paths,
                cli_names: spec.cli_names,
            })
        })
        .collect()
}

fn skill_harness_status(harness: &SkillHarness) -> SkillHarnessStatus {
    let status = agent_skill_status_at(&harness.dir);
    SkillHarnessStatus {
        id: harness.id.to_string(),
        name: harness.name.to_string(),
        detected: path_or_cli_detected(&harness.detection_paths, harness.cli_names, None),
        dir: status.dir,
        path: status.path,
        installed: status.installed,
        installed_version: status.installed_version,
        available_version: status.available_version,
        update_available: status.update_available,
    }
}

fn find_skill_harness(harness_id: &str) -> Result<SkillHarness> {
    skill_harnesses()
        .into_iter()
        .find(|harness| harness.id == harness_id)
        .ok_or_else(|| {
            let known = skill_harnesses()
                .iter()
                .map(|harness| harness.id)
                .collect::<Vec<_>>()
                .join(", ");
            anyhow!("unknown skill harness '{harness_id}' (known: {known})")
        })
}

pub fn list_skill_harnesses() -> Result<Vec<SkillHarnessStatus>> {
    Ok(skill_harnesses().iter().map(skill_harness_status).collect())
}

pub fn install_skill_harness(harness_id: &str) -> Result<SkillHarnessStatus> {
    let harness = find_skill_harness(harness_id)?;
    install_agent_skill_at(&harness.dir)?;
    Ok(skill_harness_status(&harness))
}

pub fn uninstall_skill_harness(harness_id: &str) -> Result<SkillHarnessStatus> {
    let harness = find_skill_harness(harness_id)?;
    uninstall_agent_skill_at(&harness.dir)?;
    Ok(skill_harness_status(&harness))
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

    /// `SKILL.md` points at its reference files by relative path, so an install
    /// that is missing one — or ships one nothing points at — is broken.
    #[test]
    fn every_skill_file_is_linked_and_every_link_resolves() {
        let linked: std::collections::BTreeSet<String> = EMBEDDED_SKILL_MD
            .split(|c: char| !(c.is_ascii_alphanumeric() || "/-_.".contains(c)))
            .filter(|word| word.starts_with("references/") && word.ends_with(".md"))
            .map(str::to_string)
            .collect();
        let shipped: std::collections::BTreeSet<String> = EMBEDDED_SKILL_FILES
            .iter()
            .map(|(relative, _)| relative.to_string())
            .filter(|relative| relative != "SKILL.md")
            .collect();
        assert!(!linked.is_empty(), "SKILL.md links no reference file");
        assert_eq!(
            linked, shipped,
            "SKILL.md links and the embedded files must be the same set"
        );
        for (relative, content) in EMBEDDED_SKILL_FILES {
            assert!(!content.trim().is_empty(), "{relative} is empty");
        }
        let mut seen = std::collections::BTreeSet::new();
        for (relative, _) in EMBEDDED_SKILL_FILES {
            assert!(seen.insert(*relative), "{relative} is embedded twice");
        }
    }

    /// The point of the split: what loads when the skill triggers stays small.
    /// A page of detail that belongs under `references/` creeping back into
    /// `SKILL.md` would quietly undo it.
    #[test]
    fn the_entry_file_stays_small() {
        assert!(
            EMBEDDED_SKILL_MD.len() <= 9_000,
            "SKILL.md is {} bytes; move detail into references/",
            EMBEDDED_SKILL_MD.len()
        );
    }

    #[test]
    fn install_writes_every_file_and_a_missing_one_asks_for_an_update() {
        let base = temp_dir("skill-files");
        let dir = base.join(SKILL_DIR_NAME);

        let status = install_agent_skill_at(&dir).unwrap();
        for (relative, content) in EMBEDDED_SKILL_FILES {
            assert_eq!(
                std::fs::read_to_string(dir.join(relative)).unwrap(),
                *content,
                "{relative}"
            );
        }
        assert!(status.installed && !status.update_available);

        // The same version with a reference file gone is not a complete install.
        std::fs::remove_file(dir.join("references/exec.md")).unwrap();
        assert!(agent_skill_status_at(&dir).update_available);
        // Installing again repairs it.
        assert!(!install_agent_skill_at(&dir).unwrap().update_available);

        // An install from before the skill had reference files: SKILL.md only,
        // at an older version. It is an update, and the update fills the rest.
        let legacy = base.join("legacy").join(SKILL_DIR_NAME);
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(
            legacy.join("SKILL.md"),
            "---\nname: agent2ssh\nversion: 0.0.1\n---\nold",
        )
        .unwrap();
        assert!(agent_skill_status_at(&legacy).update_available);
        install_agent_skill_at(&legacy).unwrap();
        assert!(legacy.join("references/workflows.md").exists());
        assert!(!agent_skill_status_at(&legacy).update_available);

        let _ = std::fs::remove_dir_all(&base);
    }

    /// A skill someone edited by hand, version left alone, is theirs: it must
    /// not report an update just because its text differs.
    #[test]
    fn a_hand_edited_skill_at_the_current_version_is_not_flagged() {
        let base = temp_dir("skill-edited");
        let dir = base.join(SKILL_DIR_NAME);
        install_agent_skill_at(&dir).unwrap();
        let edited = format!("{EMBEDDED_SKILL_MD}\nMy own note.\n");
        std::fs::write(dir.join("SKILL.md"), edited).unwrap();
        assert!(!agent_skill_status_at(&dir).update_available);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn uninstall_removes_every_installed_file_and_the_directories_it_emptied() {
        let base = temp_dir("skill-uninstall-all");
        let dir = base.join(SKILL_DIR_NAME);
        install_agent_skill_at(&dir).unwrap();

        assert!(uninstall_agent_skill_at(&dir).unwrap());
        assert!(!dir.exists(), "the skill directory should be gone");
        assert!(
            !uninstall_agent_skill_at(&dir).unwrap(),
            "nothing left to remove"
        );

        // A legacy single-file install is removed the same way.
        let legacy = base.join("legacy").join(SKILL_DIR_NAME);
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("SKILL.md"), "old").unwrap();
        assert!(uninstall_agent_skill_at(&legacy).unwrap());
        assert!(!legacy.exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn uninstall_keeps_files_it_did_not_install_even_inside_references() {
        let base = temp_dir("skill-keep-refs");
        let dir = base.join(SKILL_DIR_NAME);
        install_agent_skill_at(&dir).unwrap();
        std::fs::write(dir.join("references/my-notes.md"), "keep me").unwrap();

        assert!(uninstall_agent_skill_at(&dir).unwrap());
        assert!(dir.join("references/my-notes.md").exists());
        assert!(!dir.join("SKILL.md").exists());
        assert!(!dir.join("references/exec.md").exists());

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

    fn component_is(path: &Path, name: &str) -> bool {
        path.components()
            .any(|component| component.as_os_str() == name)
    }

    #[test]
    fn candidates_cover_the_editions_people_actually_install() {
        let candidates = mcp_client_candidates();
        assert!(candidates.iter().any(|c| c.id == "claude_code"));
        assert!(candidates.iter().any(|c| c.id == "trae"));
        assert!(candidates.iter().any(|c| c.id == "trae_cn"));
        assert!(candidates.iter().any(|c| c.id == "qoder"));
        assert!(candidates.iter().any(|c| c.id == "qoder_cn"));
        assert!(candidates.iter().any(|c| c.id == "agy"));
        assert!(candidates.iter().all(|c| c.id != "gemini_cli"));
        assert!(candidates.iter().all(|c| c.id != "qoder_work"));
        assert!(candidates.iter().all(|c| c.id != "trae_solo"));
        let mut ids: Vec<_> = candidates.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), candidates.len());

        let qoder = candidates.iter().find(|c| c.id == "qoder").unwrap();
        assert!(
            qoder.config_path.ends_with(".qoder/settings.json")
                || qoder.config_path.ends_with(r".qoder\settings.json")
        );
        assert!(qoder
            .detection_paths
            .iter()
            .all(|path| path.file_name().and_then(|name| name.to_str()) != Some(".qoder")));
        assert!(qoder
            .detection_paths
            .iter()
            .any(|path| path.file_name().and_then(|name| name.to_str()) == Some("settings.json")));
        assert_eq!(qoder.cli_names, &["qoder"]);

        let qoder_cn = candidates.iter().find(|c| c.id == "qoder_cn").unwrap();
        assert!(
            qoder_cn.config_path.ends_with(".qoder-cn/settings.json")
                || qoder_cn.config_path.ends_with(r".qoder-cn\settings.json")
        );
        assert!(qoder_cn.cli_names.contains(&"qodercn"));

        let config_root = dirs::config_dir().expect("config dir");
        let specs = client_specs();
        let trae_spec = specs.iter().find(|spec| spec.id == "trae").unwrap();
        assert!(trae_spec.mcp_configs[0].starts_with(&config_root));
        let trae = candidates.iter().find(|c| c.id == "trae").unwrap();
        let trae_config = trae.config_path.to_string_lossy().replace('\\', "/");
        assert!(trae_config.ends_with("Trae/User/mcp.json"), "{trae_config}");
        assert!(trae
            .detection_paths
            .iter()
            .all(|path| !component_is(path, ".trae-cn")));
        assert!(trae
            .detection_paths
            .iter()
            .any(|path| component_is(path, ".trae")));

        let trae_cn_spec = specs.iter().find(|spec| spec.id == "trae_cn").unwrap();
        assert!(trae_cn_spec.mcp_configs[0].starts_with(&config_root));
        let trae_cn = candidates.iter().find(|c| c.id == "trae_cn").unwrap();
        let trae_cn_config = trae_cn.config_path.to_string_lossy().replace('\\', "/");
        assert!(
            trae_cn_config.ends_with("Trae CN/User/mcp.json"),
            "{trae_cn_config}"
        );
        assert!(trae_cn
            .detection_paths
            .iter()
            .any(|path| component_is(path, ".trae-cn")));
        assert!(trae_cn
            .detection_paths
            .iter()
            .all(|path| !component_is(path, ".trae")));

        let agy = candidates.iter().find(|c| c.id == "agy").unwrap();
        let agy_config = agy.config_path.to_string_lossy().replace('\\', "/");
        assert!(
            agy_config.ends_with(".gemini/config/mcp_config.json")
                || agy_config.ends_with(".gemini/antigravity/mcp_config.json"),
            "{agy_config}"
        );
        assert!(agy.cli_names.contains(&"agy"));
        assert!(agy
            .detection_paths
            .iter()
            .all(|path| { path.file_name().and_then(|name| name.to_str()) != Some(".gemini") }));
    }

    #[test]
    fn select_existing_file_prefers_the_first_file_that_is_present() {
        let base = temp_dir("agy-mcp");
        let current = base.join("config").join("mcp_config.json");
        let legacy = base.join("antigravity").join("mcp_config.json");
        std::fs::create_dir_all(current.parent().unwrap()).unwrap();
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, "{}\n").unwrap();

        assert_eq!(
            select_existing_file(&[current.clone(), legacy.clone()]),
            legacy
        );

        std::fs::write(&current, "{}\n").unwrap();
        assert_eq!(
            select_existing_file(&[current.clone(), legacy.clone()]),
            current
        );

        // A directory at the preferred path is not a config file.
        let blocked = base.join("blocked.json");
        std::fs::create_dir_all(&blocked).unwrap();
        assert_eq!(select_existing_file(&[blocked, legacy.clone()]), legacy);

        let missing = base.join("missing.json");
        let also_missing = base.join("also-missing.json");
        assert_eq!(
            select_existing_file(&[missing.clone(), also_missing]),
            missing
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn select_installed_skill_dir_keeps_an_existing_install() {
        let base = temp_dir("agy-skill");
        let shared = base.join("config").join("skills").join(SKILL_DIR_NAME);
        let cli_only = base
            .join("antigravity-cli")
            .join("skills")
            .join(SKILL_DIR_NAME);
        let gemini = base.join("skills").join(SKILL_DIR_NAME);
        let candidates = [shared.clone(), cli_only.clone(), gemini.clone()];

        assert_eq!(select_installed_skill_dir(&candidates), shared);

        std::fs::create_dir_all(&gemini).unwrap();
        std::fs::write(gemini.join("SKILL.md"), "---\nversion: 1\n---\n").unwrap();
        assert_eq!(select_installed_skill_dir(&candidates), gemini);

        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(shared.join("SKILL.md"), "---\nversion: 1\n---\n").unwrap();
        assert_eq!(select_installed_skill_dir(&candidates), shared);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn user_config_candidates_keep_the_native_directory_first() {
        let parts = ["Claude", "claude_desktop_config.json"];
        let configs = user_config_candidates(&parts);
        assert_eq!(configs[0], user_config_file(&parts));
        assert!(configs.len() >= 2);
        assert!(configs.iter().all(|path| {
            path.to_string_lossy()
                .replace('\\', "/")
                .ends_with("Claude/claude_desktop_config.json")
        }));
        let again = user_config_candidates(&parts);
        assert_eq!(configs.len(), unique_paths(again).len());
    }

    #[test]
    fn order_layouts_promotes_the_install_that_is_present() {
        let base = temp_dir("layouts");
        let older = base.join("older.json");
        let newer = base.join("newer.json");
        let older_marker = base.join("older-install");
        let newer_marker = base.join("newer-install");
        let layouts = [
            Layout {
                target: older.clone(),
                markers: vec![older_marker.clone()],
            },
            Layout {
                target: newer.clone(),
                markers: vec![newer_marker.clone()],
            },
        ];

        assert_eq!(order_layouts(&layouts), vec![older.clone(), newer.clone()]);

        std::fs::create_dir_all(&newer_marker).unwrap();
        assert_eq!(order_layouts(&layouts), vec![newer.clone(), older.clone()]);
        assert_eq!(select_existing_file(&order_layouts(&layouts)), newer);

        std::fs::write(&older, "{}\n").unwrap();
        assert_eq!(select_existing_file(&order_layouts(&layouts)), older);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn codebuddy_prefers_the_current_mcp_file_and_keeps_an_older_one() {
        let base = temp_dir("codebuddy");
        let legacy = base.join(".codebuddy.json");
        let configs = codebuddy_mcp_configs(&base.join(".codebuddy"), legacy.clone());
        assert!(configs[0].ends_with(".mcp.json"));
        assert!(configs[1].ends_with("mcp.json"));
        assert_eq!(select_existing_file(&configs), configs[0]);

        std::fs::write(&legacy, "{}\n").unwrap();
        assert_eq!(select_existing_file(&configs), legacy);

        std::fs::create_dir_all(configs[0].parent().unwrap()).unwrap();
        std::fs::write(&configs[0], "{}\n").unwrap();
        assert_eq!(select_existing_file(&configs), configs[0]);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn every_client_resolves_through_the_shared_detector() {
        let harnesses = skill_harnesses();
        for spec in client_specs() {
            assert!(
                !spec.mcp_configs.is_empty(),
                "{} has no MCP candidates",
                spec.id
            );
            let resolved = select_existing_file(&spec.mcp_configs);
            let published = mcp_client_candidates()
                .into_iter()
                .find(|candidate| candidate.id == spec.id)
                .unwrap_or_else(|| panic!("missing mcp candidate {}", spec.id));
            assert_eq!(published.config_path, resolved, "{}", spec.id);

            if spec.skill_dirs.is_empty() {
                assert!(
                    harnesses.iter().all(|harness| harness.id != spec.id),
                    "{} should not install a skill",
                    spec.id
                );
            } else {
                let dir = select_installed_skill_dir(&spec.skill_dirs);
                let harness = harnesses
                    .iter()
                    .find(|harness| harness.id == spec.id)
                    .unwrap_or_else(|| panic!("missing skill harness {}", spec.id));
                assert_eq!(harness.dir, dir, "{}", spec.id);
            }
        }
    }

    #[test]
    fn skill_harnesses_use_each_clients_own_directory() {
        let harnesses = list_skill_harnesses().unwrap();
        let dir_of = |id: &str| {
            harnesses
                .iter()
                .find(|harness| harness.id == id)
                .unwrap_or_else(|| panic!("missing skill harness {id}"))
                .dir
                .clone()
        };
        assert!(
            dir_of("claude_code").ends_with(".claude/skills/agent2ssh")
                || dir_of("claude_code").ends_with(r".claude\skills\agent2ssh")
        );
        assert!(
            dir_of("qoder_cn").ends_with(".qoder-cn/skills/agent2ssh")
                || dir_of("qoder_cn").ends_with(r".qoder-cn\skills\agent2ssh")
        );
        assert!(
            dir_of("trae").ends_with(".trae/skills/agent2ssh")
                || dir_of("trae").ends_with(r".trae\skills\agent2ssh")
        );
        assert!(
            dir_of("trae_cn").ends_with(".trae-cn/skills/agent2ssh")
                || dir_of("trae_cn").ends_with(r".trae-cn\skills\agent2ssh")
        );
        let agy_skill = dir_of("agy").replace('\\', "/");
        assert!(
            agy_skill.ends_with(".gemini/config/skills/agent2ssh")
                || agy_skill.ends_with(".gemini/antigravity-cli/skills/agent2ssh")
                || agy_skill.ends_with(".gemini/skills/agent2ssh"),
            "{agy_skill}"
        );
        assert!(harnesses
            .iter()
            .all(|harness| harness.id != "claude_desktop"));
        assert!(harnesses.iter().all(|harness| harness.id != "windsurf"));
        let mut ids: Vec<_> = harnesses
            .iter()
            .map(|harness| harness.id.as_str())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), harnesses.len());
    }
}
