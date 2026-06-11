import { invoke } from "@tauri-apps/api/core";
import type { AuditEntry, ExecResult, HostProfile } from "./types";

export const api = {
  listHosts: () => invoke<HostProfile[]>("list_hosts"),
  listAudit: () => invoke<AuditEntry[]>("list_audit"),
  addHost: (host: HostProfile) => invoke<HostProfile>("add_host", { host }),
  removeHost: (name: string) => invoke<void>("remove_host", { name }),
  execSsh: (host: string, command: string, force = false) =>
    invoke<ExecResult>("exec_ssh", { request: { host, command, force } }),
};
