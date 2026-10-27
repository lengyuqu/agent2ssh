use std::future::Future;

use anyhow::Result;

use crate::{
    approval::{
        approval_mode_for_risk, approval_policy_applies_to_host, check_approval_required,
        load_approval_policies, ApprovalMode,
    },
    core::{apply_risk_override, classify_risk},
    remote::{check_daemon_scope, DaemonScope},
    risk_config::{classify_effective_risk, load_risk_rules},
    store::{append_audit_with_decision, load_config, note_human_approval},
    types::{AuditDecision, ExecResult, RiskLevel},
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
    pub side_effect: Option<String>,
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
    pub matched_policy: Option<String>,
    pub ttl_secs: u64,
    pub reason: Option<String>,
    pub change_id: Option<String>,
    pub side_effect: Option<String>,
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

/// Sources that stand for a person at the keyboard: the desktop app and the
/// CLI. Everything else (MCP, daemon, custom `AGENT2SSH_SOURCE` labels) counts
/// as AI/automation. An agent can still drive the CLI from a shell, so a
/// human-only host is a guard rail against agent clients, not a boundary
/// against a determined local process.
pub const HUMAN_SOURCES: &[&str] = &["desktop", "cli"];

/// Whether `source` may operate `host`. Hosts default to AI-allowed, and an
/// unknown host is left to the later stages to reject.
pub fn host_allows_source(host: &str, source: &str) -> bool {
    let human = HUMAN_SOURCES
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(source.trim()));
    human
        || load_config()
            .ok()
            .and_then(|config| {
                config
                    .hosts
                    .into_iter()
                    .find(|profile| profile.name == host)
            })
            .is_none_or(|profile| profile.ai_allowed)
}

/// What the authorization layer will do with commands on one host, for the
/// host card's policy summary. Computed from the live configuration, not from
/// a copy of the rules.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HostPolicySummary {
    pub host: String,
    /// `false` when AI/automation sources are refused on this host.
    pub ai_allowed: bool,
    /// Host-level risk downgrade/upgrade, if any.
    pub risk_override: Option<RiskLevel>,
    /// Approval requirement per risk level. High always needs approval unless
    /// the caller passes `force`, even when no policy asks for it.
    pub low: ApprovalMode,
    pub medium: ApprovalMode,
    pub high: ApprovalMode,
    /// Names of the approval policies that apply to this host.
    pub policies: Vec<String>,
    /// User-defined patterns that classify a command as blocked. The built-in
    /// blocked rules (destructive commands) apply on top of these.
    pub blocked_patterns: Vec<String>,
}

pub async fn host_policy_summary(host: &str) -> Result<HostPolicySummary> {
    let profile = load_config()?
        .hosts
        .into_iter()
        .find(|profile| profile.name == host)
        .ok_or_else(|| anyhow::anyhow!("host '{host}' not found"))?;
    let policies = load_approval_policies()?;
    let mode = |risk| approval_mode_for_risk(&policies, &profile.name, &profile.tags, risk);
    let high = match mode(RiskLevel::High) {
        ApprovalMode::Auto => ApprovalMode::Required,
        other => other,
    };
    Ok(HostPolicySummary {
        host: profile.name.clone(),
        ai_allowed: profile.ai_allowed,
        risk_override: profile.risk_override,
        low: mode(RiskLevel::Low),
        medium: mode(RiskLevel::Medium),
        high,
        policies: policies
            .iter()
            .filter(|policy| approval_policy_applies_to_host(policy, &profile.name, &profile.tags))
            .map(|policy| policy.name.clone())
            .collect(),
        blocked_patterns: load_risk_rules().await?.blocked.patterns,
    })
}

pub async fn effective_command_risk(command: &str) -> RiskLevel {
    classify_effective_risk(command, classify_risk(command)).await
}

pub fn command_authorization_target(host: &str) -> CommandAuthorizationTarget {
    load_config()
        .ok()
        .and_then(|config| {
            config
                .hosts
                .into_iter()
                .find(|profile| profile.name == host)
        })
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
    // Callers of this form stopped the command on policy alone, with no
    // approver involved; the approver paths record their own verdict.
    let decision = if risk == RiskLevel::Blocked {
        AuditDecision::Blocked
    } else {
        AuditDecision::Rejected
    };
    append_decided_exec_audit(
        source,
        host,
        command,
        risk,
        reason,
        change_id,
        Some((decision, "rule")),
    );
}

/// Audit a command that did not run, recording who or what stopped it.
fn append_decided_exec_audit(
    source: &str,
    host: &str,
    command: &str,
    risk: RiskLevel,
    reason: &str,
    change_id: Option<&str>,
    decision: Option<(AuditDecision, &str)>,
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
        dropped_bytes: 0,
        side_effect: None,
    };
    let _ = append_audit_with_decision(
        &result,
        risk,
        Some(reason),
        change_id,
        Some(source),
        decision,
    );
}

/// Authorize a command for call sites that have no local approval UI.
///
/// Builds the [`CommandAuthorizationInput`] and supplies an approval handler
/// that records the rejection to the audit log and fails with a composed
/// message.
///
/// `rejection_message` is the clean, operator-facing reason recorded in the
/// audit log and used as the base of the returned error. `rejection_hint` is
/// appended *only* to the returned `Err` (the interactive user hint), so the
/// audit trail stays free of instruction text. Callers pass the exact
/// user-visible wording for each surface (CLI / MCP / desktop).
// The parameter list mirrors `CommandAuthorizationInput` plus the two rejection
// strings, and it deliberately defaults `auth_scope` for the non-daemon
// callers, so passing the struct in would push that default onto all 12 of them.
#[allow(clippy::too_many_arguments)]
pub async fn authorize_command_without_approval_handler(
    source: &str,
    host: &str,
    tags: &[String],
    risk_override: Option<RiskLevel>,
    command: &str,
    force: bool,
    reason: Option<String>,
    change_id: Option<String>,
    side_effect: Option<String>,
    rejection_message: &str,
    rejection_hint: &str,
) -> Result<CommandAuthorization, CommandAuthorizationError> {
    let auth_scope = None;
    authorize_command_with_approval(
        CommandAuthorizationInput {
            auth_scope: &auth_scope,
            source,
            host,
            tags,
            risk_override,
            command,
            force,
            reason,
            change_id,
            side_effect,
        },
        |prompt| async move {
            append_rejected_exec_audit(
                &prompt.source,
                &prompt.host,
                &prompt.command,
                prompt.risk,
                rejection_message,
                prompt.change_id.as_deref(),
            );
            Err(format!("{rejection_message}{rejection_hint}"))
        },
    )
    .await
}

/// Authorize one command across every host an exec request expands to.
///
/// CLI, MCP and desktop all run the same loop: expand the host/tag selection,
/// authorize each target on its own, and keep the names of the high-risk hosts
/// that came back approved so the caller can flip `force` for exactly those.
/// The loop is where the multi-host authorization rule lives, so it is worth
/// having one copy — three copies meant three places to notice if the rule
/// changed.
///
/// The per-surface parts stay per-surface. `rejection_message` and
/// `rejection_hint` are forwarded verbatim to
/// [`authorize_command_without_approval_handler`], and rendering the returned
/// [`CommandAuthorizationError`] is left to the caller, which is also what lets
/// MCP keep its own `McpError` mapping.
///
/// A failure to expand the selection is folded into
/// [`CommandAuthorizationError::Internal`] with its original message. Every
/// surface already renders `Internal` as the bare message, so what the user
/// sees is unchanged.
// The parameter list mirrors `authorize_command_without_approval_handler` and
// has the same three callers, so passing a struct would only add a hop.
#[allow(clippy::too_many_arguments)]
pub async fn authorize_targets_without_approval_handler(
    source: &str,
    hosts: &[String],
    tags: &Option<Vec<String>>,
    command: &str,
    force: bool,
    reason: Option<String>,
    change_id: Option<String>,
    rejection_message: &str,
    rejection_hint: &str,
) -> Result<Vec<String>, CommandAuthorizationError> {
    let targets = expand_exec_authorization_targets(hosts, tags)
        .map_err(|error| CommandAuthorizationError::Internal(error.to_string()))?;
    let mut approved_hosts = Vec::new();
    for target in targets {
        let result = authorize_command_without_approval_handler(
            source,
            &target.host,
            &target.tags,
            target.risk_override,
            command,
            force,
            reason.clone(),
            change_id.clone(),
            None,
            rejection_message,
            rejection_hint,
        )
        .await?;
        if result.approved && result.risk == RiskLevel::High {
            approved_hosts.push(target.host);
        }
    }
    Ok(approved_hosts)
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
    if !host_allows_source(input.host, input.source) {
        let message = "host is restricted to human operators";
        append_decided_exec_audit(
            input.source,
            input.host,
            input.command,
            risk,
            message,
            input.change_id.as_deref(),
            Some((AuditDecision::Blocked, "rule")),
        );
        return Err(CommandAuthorizationError::Blocked {
            risk: RiskLevel::Blocked,
            message: format!("{message}: '{}'", input.host),
        });
    }
    if risk == RiskLevel::Blocked {
        let message = "command blocked by risk policy";
        append_decided_exec_audit(
            input.source,
            input.host,
            input.command,
            risk,
            message,
            input.change_id.as_deref(),
            Some((AuditDecision::Blocked, "rule")),
        );
        return Err(CommandAuthorizationError::Blocked {
            risk,
            message: format!("{message}: '{}'", input.command),
        });
    }

    let approval_policy = check_approval_required(input.host, input.tags, input.command, risk)
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
        matched_policy: approval_policy.as_ref().map(|policy| policy.name.clone()),
        ttl_secs,
        reason: input.reason,
        change_id: change_id.clone(),
        side_effect: input.side_effect.clone(),
        source: input.source.to_string(),
    };
    match approval(prompt)
        .await
        .map_err(CommandAuthorizationError::Internal)?
    {
        ApprovalOutcome::Approved => {
            note_human_approval(input.host, input.command);
            Ok(CommandAuthorization {
                risk,
                approved: true,
            })
        }
        ApprovalOutcome::Rejected => {
            append_decided_exec_audit(
                input.source,
                input.host,
                input.command,
                risk,
                "command rejected by approver",
                change_id.as_deref(),
                Some((AuditDecision::Rejected, "human")),
            );
            Err(CommandAuthorizationError::ApprovalRejected)
        }
        ApprovalOutcome::TimedOut => {
            append_decided_exec_audit(
                input.source,
                input.host,
                input.command,
                risk,
                "approval request timed out",
                change_id.as_deref(),
                Some((AuditDecision::TimedOut, "timeout")),
            );
            Err(CommandAuthorizationError::ApprovalTimedOut)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host_json(value: serde_json::Value) -> crate::types::HostProfile {
        serde_json::from_value(value).expect("host profile")
    }

    fn input_for<'a>(
        scope: &'a Option<DaemonScope>,
        source: &'a str,
        host: &'a str,
    ) -> CommandAuthorizationInput<'a> {
        CommandAuthorizationInput {
            auth_scope: scope,
            source,
            host,
            tags: &[],
            risk_override: None,
            command: "uptime",
            force: false,
            reason: None,
            change_id: None,
            side_effect: None,
        }
    }

    #[tokio::test]
    async fn human_only_host_refuses_ai_sources_and_audits_the_refusal() {
        let _dir = crate::store::TestConfigDir::new("humanonly");
        crate::store::save_config(&crate::types::AppConfig {
            hosts: vec![
                host_json(
                    serde_json::json!({"name": "vault", "host": "10.0.0.1", "ai_allowed": false}),
                ),
                host_json(serde_json::json!({"name": "web", "host": "10.0.0.2"})),
            ],
            ..Default::default()
        })
        .unwrap();
        let scope = None;

        for source in ["mcp", "daemon", "my-agent"] {
            let result =
                authorize_command_with_approval(input_for(&scope, source, "vault"), |_| async {
                    panic!("a refused host must not reach approval")
                })
                .await;
            assert!(
                matches!(result, Err(CommandAuthorizationError::Blocked { .. })),
                "{source} must be refused on a human-only host"
            );
        }
        for source in ["desktop", "CLI"] {
            let result =
                authorize_command_with_approval(input_for(&scope, source, "vault"), |_| async {
                    panic!("uptime needs no approval")
                })
                .await;
            assert!(result.is_ok(), "{source} is a human source");
        }
        // The flag is per host: the default stays open to agents.
        let result = authorize_command_with_approval(input_for(&scope, "mcp", "web"), |_| async {
            panic!("uptime needs no approval")
        })
        .await;
        assert!(result.is_ok());

        let refused = crate::store::list_audit_raw(&crate::types::AuditFilter {
            decision: Some(AuditDecision::Blocked),
            limit: 10,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(refused.len(), 3);
        assert!(refused.iter().all(|entry| entry.host == "vault"));
    }

    #[tokio::test]
    async fn host_policy_summary_reflects_live_policies() {
        let _dir = crate::store::TestConfigDir::new("policysummary");
        crate::store::save_config(&crate::types::AppConfig {
            hosts: vec![host_json(
                serde_json::json!({"name": "db-1", "host": "10.0.0.3", "tags": ["prod"], "ai_allowed": false}),
            )],
            ..Default::default()
        })
        .unwrap();
        crate::approval::save_approval_policies(&[
            crate::approval::ApprovalPolicy {
                name: "prod-medium".into(),
                hosts: vec![],
                tags: vec!["prod".into()],
                min_risk: Some(RiskLevel::Medium),
                command_pattern: None,
                requires_approval: true,
                ttl_secs: None,
            },
            crate::approval::ApprovalPolicy {
                name: "staging-only".into(),
                hosts: vec![],
                tags: vec!["staging".into()],
                min_risk: None,
                command_pattern: None,
                requires_approval: true,
                ttl_secs: None,
            },
        ])
        .unwrap();

        let summary = host_policy_summary("db-1").await.unwrap();
        assert!(!summary.ai_allowed);
        assert_eq!(summary.low, ApprovalMode::Auto);
        assert_eq!(summary.medium, ApprovalMode::Required);
        assert_eq!(summary.high, ApprovalMode::Required);
        assert_eq!(summary.policies, vec!["prod-medium".to_string()]);
        assert!(host_policy_summary("missing").await.is_err());
    }

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
            side_effect: None,
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
            side_effect: None,
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
            side_effect: None,
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
