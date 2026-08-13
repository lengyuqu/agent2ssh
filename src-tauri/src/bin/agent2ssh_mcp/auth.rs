use agent2ssh::execution_control::{
    append_rejected_exec_audit, authorize_command_with_approval, command_authorization_target,
    expand_exec_authorization_targets, CommandAuthorizationError, CommandAuthorizationInput,
};
use agent2ssh::{dry_run_playbook, list_playbooks_core, ExecRequest, RiskLevel};
use std::collections::HashMap;

use super::McpError;

pub(super) async fn authorize_local_mcp_exec_request(
    request: &mut ExecRequest,
) -> std::result::Result<RiskLevel, McpError> {
    let target = command_authorization_target(&request.host);
    let source = request.source.as_deref().unwrap_or("mcp").to_string();
    let auth_scope = None;
    let result = authorize_command_with_approval(
        CommandAuthorizationInput {
            auth_scope: &auth_scope,
            source: &source,
            host: &request.host,
            tags: &target.tags,
            risk_override: target.risk_override,
            command: &request.command,
            force: request.force,
            reason: request.reason.clone(),
            change_id: request.change_id.clone(),
            side_effect: request.side_effect.clone(),
        },
        |prompt| async move {
            let message = "approval required but no local MCP approval handler is available";
            append_rejected_exec_audit(
                &prompt.source,
                &prompt.host,
                &prompt.command,
                prompt.risk,
                message,
                prompt.change_id.as_deref(),
            );
            Err(format!("{message}; run through the daemon approval flow"))
        },
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
    let targets = expand_exec_authorization_targets(hosts, tags).map_err(McpError::from)?;
    let auth_scope = None;
    let mut approved_hosts = Vec::new();
    for target in targets {
        let result = authorize_command_with_approval(
            CommandAuthorizationInput {
                auth_scope: &auth_scope,
                source,
                host: &target.host,
                tags: &target.tags,
                risk_override: target.risk_override,
                command,
                force,
                reason: reason.clone(),
                change_id: change_id.clone(),
                side_effect: None,
            },
            |prompt| async move {
                let message = "approval required but no local MCP approval handler is available";
                append_rejected_exec_audit(
                    &prompt.source,
                    &prompt.host,
                    &prompt.command,
                    prompt.risk,
                    message,
                    prompt.change_id.as_deref(),
                );
                Err(format!("{message}; run through the daemon approval flow"))
            },
        )
        .await
        .map_err(mcp_authorization_error)?;
        if result.approved && result.risk == RiskLevel::High {
            approved_hosts.push(target.host);
        }
    }
    Ok(approved_hosts)
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
    let auth_scope = None;
    let mut approved_steps = Vec::new();

    for step in dry_run.steps {
        let result = authorize_command_with_approval(
            CommandAuthorizationInput {
                auth_scope: &auth_scope,
                source,
                host,
                tags: &target.tags,
                risk_override,
                command: &step.command_resolved,
                force,
                reason: reason.clone(),
                change_id: change_id.clone(),
                side_effect: None,
            },
            |prompt| async move {
                let message = "approval required but no local MCP approval handler is available";
                append_rejected_exec_audit(
                    &prompt.source,
                    &prompt.host,
                    &prompt.command,
                    prompt.risk,
                    message,
                    prompt.change_id.as_deref(),
                );
                Err(format!("{message}; run through the daemon approval flow"))
            },
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
    let auth_scope = None;
    authorize_command_with_approval(
        CommandAuthorizationInput {
            auth_scope: &auth_scope,
            source,
            host,
            tags: &target.tags,
            risk_override: target.risk_override,
            command,
            force,
            reason: None,
            change_id: None,
            side_effect: None,
        },
        |prompt| async move {
            let message = "approval required but no local MCP approval handler is available";
            append_rejected_exec_audit(
                &prompt.source,
                &prompt.host,
                &prompt.command,
                prompt.risk,
                message,
                prompt.change_id.as_deref(),
            );
            Err(format!("{message}; run through the daemon approval flow"))
        },
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
