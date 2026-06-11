import { invoke } from "@tauri-apps/api/core";
import type {
  AuditEntry,
  AuditFilter,
  ExecMultiResult,
  ExecRequest,
  ExecResult,
  ForwardDirection,
  ForwardRule,
  HostProfile,
  PingResult,
  RiskLevel,
  SessionInfo,
  SftpResult,
} from "./types";

export const api = {
  // Host management
  listHosts: () => invoke<HostProfile[]>("list_hosts"),
  addHost: (host: HostProfile) => invoke<HostProfile>("add_host", { host }),
  removeHost: (name: string) => invoke<void>("remove_host", { name }),
  importSshConfig: (path?: string) =>
    invoke<HostProfile[]>("import_ssh_config", { path: path ?? null }),

  // Risk classification
  classifyRisk: (command: string) =>
    invoke<RiskLevel>("classify_command_risk", { command }),

  // Execution
  execSsh: (host: string, command: string, force = false) =>
    invoke<ExecResult>("exec_ssh", { request: { host, command, force } }),
  execSshFull: (request: ExecRequest) =>
    invoke<ExecResult>("exec_ssh", { request }),
  execMulti: (
    hosts: string[],
    command: string,
    force = false,
    timeoutSecs?: number
  ) =>
    invoke<ExecMultiResult[]>("exec_multi", {
      hosts,
      command,
      force,
      timeoutSecs: timeoutSecs ?? null,
    }),
  pingHosts: (hosts: string[], timeoutSecs?: number) =>
    invoke<PingResult[]>("ping_hosts", {
      hosts,
      timeoutSecs: timeoutSecs ?? null,
    }),

  // SFTP
  sftpUpload: (host: string, localPath: string, remotePath: string) =>
    invoke<SftpResult>("sftp_upload", {
      request: { host, local_path: localPath, remote_path: remotePath },
    }),
  sftpDownload: (host: string, remotePath: string, localPath: string) =>
    invoke<SftpResult>("sftp_download", {
      request: { host, remote_path: remotePath, local_path: localPath },
    }),
  sftpLs: (host: string, path: string, timeoutSecs?: number) =>
    invoke<ExecResult>("sftp_ls", {
      host,
      path,
      timeoutSecs: timeoutSecs ?? null,
    }),
  sftpStat: (host: string, path: string, timeoutSecs?: number) =>
    invoke<ExecResult>("sftp_stat", {
      host,
      path,
      timeoutSecs: timeoutSecs ?? null,
    }),
  sftpMkdir: (host: string, path: string, timeoutSecs?: number) =>
    invoke<ExecResult>("sftp_mkdir", {
      host,
      path,
      timeoutSecs: timeoutSecs ?? null,
    }),

  // Sessions
  sessionOpen: (host: string) => invoke<string>("session_open", { host }),
  sessionWrite: (id: string, input: string) =>
    invoke<void>("session_write", { id, input }),
  sessionRead: (id: string, timeoutMs?: number) =>
    invoke<string>("session_read", { id, timeoutMs: timeoutMs ?? 2000 }),
  sessionClose: (id: string) => invoke<void>("session_close", { id }),
  sessionList: () => invoke<SessionInfo[]>("session_list"),

  // Port forwarding
  forwardAdd: (
    host: string,
    direction: ForwardDirection,
    bindPort: number,
    targetHost: string,
    targetPort: number
  ) =>
    invoke<ForwardRule>("forward_add", {
      host,
      direction,
      bindPort,
      targetHost,
      targetPort,
    }),
  forwardList: () => invoke<ForwardRule[]>("forward_list"),
  forwardRemove: (id: string) => invoke<void>("forward_remove", { id }),

  // Audit
  listAudit: (filter?: AuditFilter) =>
    invoke<AuditEntry[]>("list_audit", { filter: filter ?? null }),
};
