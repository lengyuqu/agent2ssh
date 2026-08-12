pub mod anomaly;
pub mod app_state;
pub mod approval;
pub mod backup_crypto;
pub mod config_cache;
pub mod connection;
pub mod core;
pub mod daemon_control;
pub mod diagnostics;
pub mod embedded_ssh;
pub mod error_codes;
pub mod events;
pub mod execution_control;
pub mod forward;
pub mod gate;
pub mod health;
pub mod integrate;
pub mod jump_chain;
pub mod keys;
pub mod lifecycle;
pub mod limits;
pub mod mcp_binding;
pub mod notify;
pub mod path_resolver;
pub mod playbook;
pub mod policy;
pub mod redaction;
pub mod remote;
pub mod risk_config;
pub mod sanitize;
pub mod secrets;
pub mod session;
pub mod sftp_transfer;
pub mod ssh_algo;
pub mod store;
pub mod telemetry;
pub mod types;
pub mod webdav_sync;
pub mod ws_drain;
pub mod prompt_waiter;
pub mod url_safety;
pub mod snippets;
pub mod container_discovery;
pub mod osc_ipc;
pub mod copy_redact;

#[cfg(feature = "tauri")]
pub mod tauri_commands;

pub use anomaly::{
    detect_anomalies, load_anomaly_config, publish_anomalies, AnomalyConfig, AnomalyFinding,
    AnomalyKind, AnomalySeverity,
};
pub use approval::{
    approval_action_url, approval_request_with_context, build_approval_context,
    build_approval_context_with_effective_risk, check_approval_required, list_approval_policies,
    load_approval_policies, save_approval_policies, ApprovalContext, ApprovalHistoryEntry,
    ApprovalPolicy, ApprovalPolicyFile, RiskDetails,
};
pub use connection::{connect_host, disconnect_host, list_active_connections};
pub use core::{
    add_host_core, build_plan_from_profile, classify_risk, compare_exec_results,
    compare_ssh_configs, delete_host_group_core, delete_proxy_core, exec_multi_core,
    exec_multi_with_strategy, exec_ssh_core, export_team_config, export_to_ssh_config,
    export_to_ssh_config_format, filter_hosts, import_ssh_config_core, import_team_config,
    list_audit_core, list_host_groups_core, list_hosts_core, list_hosts_filtered_core,
    list_proxies_core, ping_hosts_core, preview_exec, preview_exec_multi,
    preview_team_config_import, remove_host_core, save_host_group_core, save_proxy_core,
    sftp_download_core, sftp_download_core_with_source, sftp_ls_core, sftp_ls_core_with_source,
    sftp_mkdir_core, sftp_mkdir_core_with_source, sftp_stat_core, sftp_stat_core_with_source,
    sftp_upload_core, sftp_upload_core_with_source, update_host_core, ConfigDiffPreview,
    ExecComparison, ExecMultiBatchRequest, ExecMultiRequest, ExecPlan, ExecPlanTarget,
    ExitCodeGroup, ImportResult, OutputComparison, OutputDiff, SshSyncDiff, SshSyncHostConflict,
    SshSyncHostDiff, SshSyncStrategy, TeamConfigExport,
};
pub use diagnostics::{
    app_log_path, append_diagnostic_log, append_diagnostic_log_no_sink, clear_diagnostic_logs,
    current_trace_id, export_diagnostic_bundle, install_panic_hook, list_diagnostic_logs,
    seed_trace_id_from_env, set_error_sink, set_trace_id, DiagnosticLogEntry,
};
pub use events::{event_bus, publish_event, subscribe_events, Agent2SSHEvent, EventType};
pub use execution_control::{
    append_rejected_exec_audit, authorize_command_with_approval, command_authorization_target,
    effective_command_risk, expand_exec_authorization_targets, expand_exec_targets,
    ApprovalOutcome, ApprovalPrompt, CommandAuthorization, CommandAuthorizationError,
    CommandAuthorizationInput, CommandAuthorizationTarget,
};
pub use error_codes::{
    is_coded_error, parse_coded_error, AnyhowToCodedExt, CodedError, CodedResult, ErrorCode,
    WIRE_PREFIX as ERROR_WIRE_PREFIX,
};
pub use forward::{forward_add_core, forward_list_core, forward_remove_core, forward_stats_core};
pub use gate::{
    execution_gate_blocks_source, gate_blocks_source, load_execution_gate, save_execution_gate,
    source_can_bypass_gate, ExecutionGateMode, ExecutionGateStatus,
};
pub use health::{
    collect_health_snapshot, load_health_snapshot, HealthSnapshot, HostHealthSnapshot,
};
pub use limits::{
    load_execution_limits, ExecutionLimitConfig, ExecutionLimitRejection, ExecutionLimitRule,
    ExecutionLimiter,
};
pub use mcp_binding::{
    create_mcp_binding_key, mcp_binding_key_is_valid, verify_mcp_binding_from_env,
    MCP_BINDING_KEY_ENV, MCP_SOURCE_ENV,
};
pub use playbook::{
    delete_playbook_core, dry_run_playbook, list_playbooks_core, resolve_command_template,
    run_playbook_core, run_playbook_core_with_source,
    run_playbook_core_with_source_and_approved_steps, save_playbook_core, validate_playbook_params,
    DryRunStep, Playbook, PlaybookDryRun, PlaybookParam, PlaybookRunResult, PlaybookStep,
    PlaybookStepResult,
};
pub use policy::{
    existing_policy_path, load_policy_file, load_policy_from_path, parse_policy, policy_json_path,
    policy_toml_path, validate_policy_path, AgentPolicyFile, PolicyDecision, PolicyTestResult,
};
pub use redaction::{
    default_rules, load_rules_from_json, redact_default, redact_with_defaults, redact_with_rules,
    validate_pattern, RedactRule, RedactRuleConfig, RedactRuleError,
};
pub use remote::{
    check_daemon_version, check_version_compatibility, diagnose_daemon, get_daemons_unified_view,
    is_loopback_addr, list_daemons_core, load_remotes, local_daemon_addr,
    local_daemon_connect_addr, local_daemon_url, remote_host_tags, tags_for_remote_scope_check,
    DaemonDiagnostic, DaemonHealthSummary, DaemonInfo, DaemonMetricsSummary, DaemonUnifiedView,
    DaemonViewEntry, DiagnosticCheck, DiagnosticStatus, RemoteDaemon, VersionCompatibility,
    DEFAULT_DAEMON_ADDR, PROTOCOL_VERSION,
};
pub use session::{
    session_close_core, session_list_core, session_open_core, session_read_core, session_write_core,
};
pub use store::{
    compute_metrics_trend, config_dir, export_audit_csv, export_audit_jsonl,
    migrate_plaintext_secrets, HostExecutionCount, HourlyBucket, MetricsTrend, RiskDistribution,
    TrendPeriod,
};
pub use telemetry::{
    load_telemetry_config, record_event, save_telemetry_config, telemetry_enabled, TelemetryConfig,
};
pub use types::*;
pub use webdav_sync::{
    collect_sync_files, create_sync_backup, load_local_sync_marker, webdav_pull, webdav_push,
    webdav_status, WebDavSyncFile, WebDavSyncMarker, WebDavSyncOptions, WebDavSyncResult,
    WebDavSyncStatus, SYNCABLE_FILES,
};

pub use app_state::{
    app_state, host, lifecycle, set_host, AppState, Host, ResourceKind, ResourceOwner,
    ResourcePhase, ResourceRecord,
};
pub use lifecycle::{
    LifecycleError, LifecycleRegistry, ResourceReservation,
};
pub use backup_crypto::{
    decrypt_backup, encrypt_backup, is_encrypted_backup, ENCRYPTED_MAGIC,
};
pub use path_resolver::resolve_executable_in;
pub use sftp_transfer::walk_local_dir;
pub use url_safety::{validate_url_scheme, open_external_url, strip_ansi_escapes};
pub use snippets::{load_snippets, save_snippets, Snippet};
pub use container_discovery::{discover_containers, ContainerDiscoveryTarget, ContainerPlatform};
pub use osc_ipc::{emit_osc_open, emit_osc_forward, AGENT2SSH_APP_ENV};
pub use copy_redact::{redact_for_clipboard, load_copy_redact_rules, save_copy_redact_rules, CopyRedactRule};

#[cfg(feature = "tauri")]
pub use tauri_commands::run_tauri;
