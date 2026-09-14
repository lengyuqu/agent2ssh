use agent2ssh::execution_control::{
    authorize_command_without_approval_handler, authorize_targets_without_approval_handler,
    command_authorization_target, CommandAuthorizationError,
};
use agent2ssh::{dry_run_playbook, list_playbooks_core, ExecRequest, RiskLevel};
use std::collections::HashMap;

use super::McpError;

pub(super) async fn authorize_local_mcp_exec_request(
    request: &mut ExecRequest,
) -> std::result::Result<RiskLevel, McpError> {
    let target = command_authorization_target(&request.host);
    let source = request.source.as_deref().unwrap_or("mcp").to_string();
    let result = authorize_command_without_approval_handler(
        &source,
        &request.host,
        &target.tags,
        target.risk_override,
        &request.command,
        request.force,
        request.reason.clone(),
        request.change_id.clone(),
        request.side_effect.clone(),
        "approval required but no local MCP approval handler is available",
        "; run through the daemon approval flow",
    )
    .await
    .map_err(mcp_authorization_error)?;
    if result.approved && result.risk == RiskLevel::High {
        request.force = true;
    }
    Ok(result.risk)
}

pub(super) async fn authorize_local_mcp_exec_targets(
    hosts: &[String],
    tags: &Option<Vec<String>>,
    command: &str,
    force: bool,
    reason: Option<String>,
    change_id: Option<String>,
    source: &str,
) -> std::result::Result<Vec<String>, McpError> {
    authorize_targets_without_approval_handler(
        source,
        hosts,
        tags,
        command,
        force,
        reason,
        change_id,
        "approval required but no local MCP approval handler is available",
        "; run through the daemon approval flow",
    )
    .await
    .map_err(mcp_authorization_error)
}

pub(super) async fn authorize_local_mcp_playbook_run(
    playbook: &str,
    host: &str,
    force: bool,
    params: &HashMap<String, String>,
    reason: Option<String>,
    change_id: Option<String>,
    source: &str,
) -> std::result::Result<Vec<usize>, McpError> {
    let dry_run = dry_run_playbook(playbook, params).map_err(McpError::from)?;
    let target = command_authorization_target(host);
    let playbook_risk_override = list_playbooks_core()
        .map_err(McpError::from)?
        .into_iter()
        .find(|item| item.name == playbook)
        .and_then(|item| item.risk_override);
    let risk_override = playbook_risk_override.or(target.risk_override);
    let mut approved_steps = Vec::new();

    for step in dry_run.steps {
        let result = authorize_command_without_approval_handler(
            source,
            host,
            &target.tags,
            risk_override,
            &step.command_resolved,
            force,
            reason.clone(),
            change_id.clone(),
            None,
            "approval required but no local MCP approval handler is available",
            "; run through the daemon approval flow",
        )
        .await
        .map_err(mcp_authorization_error)?;
        if result.approved && result.risk == RiskLevel::High {
            approved_steps.push(step.step);
        }
    }

    Ok(approved_steps)
}

pub(super) async fn authorize_local_mcp_operation(
    host: &str,
    command: &str,
    force: bool,
    source: &str,
) -> std::result::Result<(), McpError> {
    let target = command_authorization_target(host);
    authorize_command_without_approval_handler(
        source,
        host,
        &target.tags,
        target.risk_override,
        command,
        force,
        None,
        None,
        None,
        "approval required but no local MCP approval handler is available",
        "; run through the daemon approval flow",
    )
    .await
    .map_err(mcp_authorization_error)?;
    Ok(())
}

fn mcp_authorization_error(error: CommandAuthorizationError) -> McpError {
    match error {
        CommandAuthorizationError::ScopeDenied(message) => McpError::internal(message),
        CommandAuthorizationError::Blocked { message, .. } => McpError::internal(message),
        CommandAuthorizationError::ApprovalRejected => {
            McpError::internal("command rejected by approver")
        }
        CommandAuthorizationError::ApprovalTimedOut => {
            McpError::internal("approval request timed out")
        }
        CommandAuthorizationError::Internal(message) => McpError::internal(message),
    }
}
