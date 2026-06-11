pub mod approval;
pub mod connection;
pub mod core;
pub mod forward;
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
pub use core::{
    add_host_core, classify_risk, exec_multi_core, exec_ssh_core, import_ssh_config_core,
    list_audit_core, list_hosts_core, ping_hosts_core, remove_host_core, sftp_download_core,
    sftp_ls_core, sftp_mkdir_core, sftp_stat_core, sftp_upload_core,
};
pub use forward::{forward_add_core, forward_list_core, forward_remove_core};
pub use playbook::{list_playbooks_core, run_playbook_core, Playbook, PlaybookRunResult, PlaybookStepResult};
pub use remote::{list_daemons_core, load_remotes, DaemonInfo, RemoteDaemon};
pub use session::{
    session_close_core, session_list_core, session_open_core, session_read_core, session_write_core,
};
pub use store::config_dir;
pub use types::*;

#[cfg(feature = "tauri")]
pub use tauri_commands::run_tauri;
