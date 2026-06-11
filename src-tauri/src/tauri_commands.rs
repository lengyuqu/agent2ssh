use crate::{
    core::{add_host_core, exec_ssh_core, list_audit_core, list_hosts_core, remove_host_core},
    types::{AuditEntry, ExecRequest, ExecResult, HostProfile},
};

#[tauri::command]
pub async fn exec_ssh(request: ExecRequest) -> Result<ExecResult, String> {
    exec_ssh_core(request).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_hosts() -> Result<Vec<HostProfile>, String> {
    list_hosts_core().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_host(host: HostProfile) -> Result<HostProfile, String> {
    add_host_core(host).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_host(name: String) -> Result<(), String> {
    remove_host_core(&name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_audit() -> Result<Vec<AuditEntry>, String> {
    list_audit_core(50).map_err(|e| e.to_string())
}

pub fn run_tauri() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            exec_ssh,
            list_hosts,
            add_host,
            remove_host,
            list_audit
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
