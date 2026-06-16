pub mod approval;
pub mod connection;
pub mod core;
pub mod events;
pub mod forward;
pub mod health;
pub mod keys;
pub mod notify;
pub mod playbook;
pub mod remote;
pub mod risk_config;
pub mod session;
pub mod store;
pub mod types;

#[cfg(feature = "tauri")]
pub mod tauri_commands;

pub use connection::{connect_host, disconnect_host, list_active_connections};
pub use approval::{
    ApprovalPolicy, ApprovalPolicyFile, check_approval_required,
    load_approval_policies, save_approval_policies, list_approval_policies,
    ApprovalContext, ApprovalHistoryEntry, RiskDetails,
    approval_request_with_context, approval_action_url, build_approval_context,
};
pub use core::{
    add_host_core, build_plan_from_profile, classify_risk, compare_exec_results, compare_ssh_configs,
    exec_multi_core, exec_multi_with_strategy, exec_ssh_core, export_team_config,
    export_to_ssh_config, export_to_ssh_config_format, filter_hosts,
    import_ssh_config_core, import_team_config, list_audit_core, list_hosts_core,
    list_hosts_filtered_core, ping_hosts_core, preview_exec, preview_exec_multi,
    preview_team_config_import, remove_host_core, sftp_download_core, sftp_ls_core,
    sftp_mkdir_core, sftp_stat_core, sftp_upload_core, ConfigDiffPreview, ExecComparison,
    ExecPlan, ExecPlanTarget, ExitCodeGroup, ImportResult, OutputComparison, OutputDiff,
    SshSyncDiff, SshSyncHostConflict, SshSyncHostDiff, SshSyncStrategy,
    TeamConfigExport,
};
pub use events::{
    event_bus, publish_event, subscribe_events, Agent2SSHEvent, EventType,
};
pub use forward::{forward_add_core, forward_list_core, forward_remove_core};
pub use health::{collect_health_snapshot, load_health_snapshot, HealthSnapshot, HostHealthSnapshot};
pub use playbook::{
    dry_run_playbook, list_playbooks_core, resolve_command_template, run_playbook_core,
    run_playbook_core_with_source, validate_playbook_params, DryRunStep, Playbook, PlaybookDryRun,
    PlaybookParam, PlaybookRunResult, PlaybookStep, PlaybookStepResult,
};
pub use remote::{
    check_daemon_version, check_version_compatibility, diagnose_daemon, get_daemons_unified_view,
    list_daemons_core, load_remotes, DaemonDiagnostic, DaemonHealthSummary, DaemonInfo,
    DaemonMetricsSummary, DaemonUnifiedView, DaemonViewEntry, DiagnosticCheck, DiagnosticStatus,
    RemoteDaemon, VersionCompatibility, PROTOCOL_VERSION,
};
pub use session::{
    session_close_core, session_list_core, session_open_core, session_read_core, session_write_core,
};
pub use store::{
    compute_metrics_trend, config_dir, export_audit_csv, export_audit_jsonl, HostExecutionCount,
    HourlyBucket, MetricsTrend, RiskDistribution, TrendPeriod,
};
pub use types::*;

#[cfg(feature = "tauri")]
pub use tauri_commands::run_tauri;
