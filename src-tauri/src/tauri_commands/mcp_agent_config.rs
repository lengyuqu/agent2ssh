//! Thin Tauri wrappers over `crate::integrate` — MCP client registration and
//! Agent Skill install/update/uninstall for the desktop MCP Agents panel.

pub use crate::integrate::{
    AgentSkillStatus, McpAgentConfigStatus, McpAgentConfigureResult, McpAgentUninstallResult,
};

#[tauri::command]
pub fn list_mcp_agent_configs() -> Result<Vec<McpAgentConfigStatus>, String> {
    crate::integrate::list_mcp_client_configs().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn configure_mcp_agent(agent_id: String) -> Result<McpAgentConfigureResult, String> {
    crate::integrate::configure_mcp_client(&agent_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn uninstall_mcp_agent(agent_id: String) -> Result<McpAgentUninstallResult, String> {
    crate::integrate::uninstall_mcp_client(&agent_id).map_err(|e| e.to_string())
}

/// V5: status of the bundled Agent Skill in the Claude Code skills directory.
#[tauri::command]
pub fn agent_skill_status() -> Result<AgentSkillStatus, String> {
    crate::integrate::agent_skill_status().map_err(|e| e.to_string())
}

/// V5: install or update (same operation) the bundled Agent Skill.
#[tauri::command]
pub fn install_agent_skill() -> Result<AgentSkillStatus, String> {
    crate::integrate::install_agent_skill().map_err(|e| e.to_string())
}

/// V5: remove the installed Agent Skill; returns whether anything was removed.
#[tauri::command]
pub fn uninstall_agent_skill() -> Result<AgentSkillStatus, String> {
    crate::integrate::uninstall_agent_skill().map_err(|e| e.to_string())?;
    crate::integrate::agent_skill_status().map_err(|e| e.to_string())
}
