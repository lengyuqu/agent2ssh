use std::future::Future;

use anyhow::Result;

use crate::{
    approval::check_approval_required,
    core::{apply_risk_override, classify_risk},
    remote::{check_daemon_scope, DaemonScope},
    risk_config::classify_effective_risk,
    store::{append_audit, load_config},
    types::{ExecResult, RiskLevel},
};

#[derive(Debug, Clone)]
pub struct CommandAuthorizationInput<'a> {
    pub auth_scope: &'a Option<DaemonScope>,
    pub source: &'a str,
    pub host: &'a str,
    pub tags: &'a [String],
    pub risk_override: Option<RiskLevel>,
    pub command: &'a str,
    pub force: bool,
    pub reason: Option<String>,
    pub change_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandAuthorization {
    pub risk: RiskLevel,
    pub approved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandAuthorizationTarget {
    pub host: String,
    pub tags: Vec<String>,
    pub risk_override: Option<RiskLevel>,
}

#[derive(Debug, Clone)]
pub struct ApprovalPrompt {
    pub host: String,
    pub command: String,
    pub risk: RiskLevel,
    pub ttl_secs: u64,
    pub reason: Option<String>,
    pub change_id: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalOutcome {
    Approved,
    Rejected,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandAuthorizationError {
    ScopeDenied(String),
    Blocked { risk: RiskLevel, message: String },
    ApprovalRejected,
    ApprovalTimedOut,
    Internal(String),
}

pub async fn effective_command_risk(command: &str) -> RiskLevel {
    classify_effective_risk(command, classify_risk(command)).await
}

pub fn command_authorization_target(host: &str) -> CommandAuthorizationTarget {
    load_config()
        .ok()
        .and_then(|config| config.hosts.into_iter().find(|profile| profile.name == host))
        .map(|profile| CommandAuthorizationTarget {
            host: profile.name,
            tags: profile.tags,
            risk_override: profile.risk_override,
        })
        .unwrap_or_else(|| CommandAuthorizationTarget {
            host: host.to_string(),
            tags: Vec::new(),
            risk_override: None,
        })
}

pub fn expand_exec_authorization_targets(
    hosts: &[String],
    tags: &Option<Vec<String>>,
) -> Result<Vec<CommandAuthorizationTarget>> {
    let config = load_config()?;
    let mut expanded: Vec<CommandAuthorizationTarget> = Vec::new();
    for host in hosts {
        if let Some(profile) = config.hosts.iter().find(|profile| profile.name == *host) {
            expanded.push(CommandAuthorizationTarget {
                host: profile.name.clone(),
                tags: profile.tags.clone(),
                risk_override: profile.risk_override,
            });
        } else {
            expanded.push(CommandAuthorizationTarget {
                host: host.clone(),
                tags: Vec::new(),
                risk_override: None,
            });
        }
    }

    if let Some(tag_list) = tags {
        if !tag_list.is_empty() {
            for profile in &config.hosts {
                if profile.tags.iter().any(|tag| tag_list.contains(tag))
                    && !expanded.iter().any(|target| target.host == profile.name)
                {
                    expanded.push(CommandAuthorizationTarget {
                        host: profile.name.clone(),
                        tags: profile.tags.clone(),
                        risk_override: profile.risk_override,
                    });
                }
            }
        }
    }

    Ok(expanded)
}

pub fn expand_exec_targets(
    hosts: &[String],
    tags: &Option<Vec<String>>,
) -> Result<Vec<(String, Vec<String>)>> {
    expand_exec_authorization_targets(hosts, tags).map(|targets| {
        targets
            .into_iter()
            .map(|target| (target.host, target.tags))
            .collect()
    })
}

pub fn append_rejected_exec_audit(
    source: &str,
    host: &str,
    command: &str,
    risk: RiskLevel,
    reason: &str,
    change_id: Option<&str>,
) {
    let result = ExecResult {
        host: host.to_string(),
        command: command.to_string(),
        exit_code: None,
        stdout: String::new(),
        stderr: reason.to_string(),
        duration_ms: 0,
        risk_level: risk,
        truncated: false,
    };
    let _ = append_audit(&result, risk, Some(reason), change_id, Some(source));
}

pub async fn authorize_command_with_approval<F, Fut>(
    input: CommandAuthorizationInput<'_>,
    approval: F,
) -> Result<CommandAuthorization, CommandAuthorizationError>
where
    F: FnOnce(ApprovalPrompt) -> Fut,
    Fut: Future<Output = Result<ApprovalOutcome, String>>,
{
    check_daemon_scope(input.auth_scope, input.host, input.tags, input.command)
        .map_err(CommandAuthorizationError::ScopeDenied)?;

    let risk = apply_risk_override(
        effective_command_risk(input.command).await,
        input.risk_override,
    );
    if risk == RiskLevel::Blocked {
        let message = "command blocked by risk policy";
        append_rejected_exec_audit(
            input.source,
            input.host,
            input.command,
            risk,
            message,
            input.change_id.as_deref(),
        );
        return Err(CommandAuthorizationError::Blocked {
            risk,
            message: format!("{message}: '{}'", input.command),
        });
    }

    let approval_policy =
        check_approval_required(input.host, input.tags, input.command, risk)
            .map_err(|e| CommandAuthorizationError::Internal(e.to_string()))?;
    let needs_approval = approval_policy.is_some() || (risk == RiskLevel::High && !input.force);
    if !needs_approval {
        return Ok(CommandAuthorization {
            risk,
            approved: false,
        });
    }

    let ttl_secs = approval_policy
        .as_ref()
        .and_then(|policy| policy.ttl_secs)
        .unwrap_or(300);
    let change_id = input.change_id.clone();
    let prompt = ApprovalPrompt {
        host: input.host.to_string(),
        command: input.command.to_string(),
        risk,
        ttl_secs,
        reason: input.reason,
        change_id: change_id.clone(),
        source: input.source.to_string(),
    };
    match approval(prompt)
        .await
        .map_err(CommandAuthorizationError::Internal)?
    {
        ApprovalOutcome::Approved => Ok(CommandAuthorization {
            risk,
            approved: true,
        }),
        ApprovalOutcome::Rejected => {
            append_rejected_exec_audit(
                input.source,
                input.host,
                input.command,
                risk,
                "command rejected by approver",
                change_id.as_deref(),
            );
            Err(CommandAuthorizationError::ApprovalRejected)
        }
        ApprovalOutcome::TimedOut => {
            append_rejected_exec_audit(
                input.source,
                input.host,
                input.command,
                risk,
                "approval request timed out",
                change_id.as_deref(),
            );
            Err(CommandAuthorizationError::ApprovalTimedOut)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scope_denial_happens_before_approval() {
        let scope = Some(DaemonScope {
            allowed_hosts: vec!["prod".to_string()],
            ..DaemonScope::default()
        });
        let input = CommandAuthorizationInput {
            auth_scope: &scope,
            source: "test",
            host: "dev",
            tags: &[],
            risk_override: None,
            command: "uptime",
            force: false,
            reason: None,
            change_id: None,
        };
        let result = authorize_command_with_approval(input, |_| async {
            panic!("approval should not be requested when scope denies")
        })
        .await;
        assert!(matches!(
            result,
            Err(CommandAuthorizationError::ScopeDenied(_))
        ));
    }

    #[tokio::test]
    async fn risk_override_is_applied_before_approval() {
        let auth_scope = None;
        let input = CommandAuthorizationInput {
            auth_scope: &auth_scope,
            source: "test",
            host: "prod",
            tags: &[],
            risk_override: Some(RiskLevel::Low),
            command: "sudo whoami",
            force: false,
            reason: None,
            change_id: None,
        };

        let result = authorize_command_with_approval(input, |_| async {
            panic!("approval should not be requested after trusted downgrade")
        })
        .await
        .unwrap();

        assert_eq!(result.risk, RiskLevel::Low);
        assert!(!result.approved);
    }

    #[tokio::test]
    async fn risk_override_cannot_unblock_blocked_command() {
        let auth_scope = None;
        let input = CommandAuthorizationInput {
            auth_scope: &auth_scope,
            source: "test",
            host: "prod",
            tags: &[],
            risk_override: Some(RiskLevel::Low),
            command: "rm -rf /",
            force: true,
            reason: None,
            change_id: None,
        };

        let result = authorize_command_with_approval(input, |_| async {
            panic!("approval should not be requested for blocked command")
        })
        .await;

        assert!(matches!(
            result,
            Err(CommandAuthorizationError::Blocked {
                risk: RiskLevel::Blocked,
                ..
            })
        ));
    }
}
