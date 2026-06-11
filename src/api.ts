import { invoke } from "@tauri-apps/api/core";
import type {
  ApprovalRequest,
  AuditEntry,
  AuditFilter,
  ConnectionStatus,
  DaemonInfo,
  ExecMultiResult,
  ExecRequest,
  ExecResult,
  ForwardDirection,
  ForwardRule,
  HostProfile,
  PingResult,
  Playbook,
  PlaybookRunResult,
  RiskLevel,
  SessionInfo,
  SftpResult,
  SshKeyInfo,
  WebhookConfig,
} from "./types";

let daemonUrl = "http://127.0.0.1:7722";

/** Change the base URL used for direct daemon HTTP calls (e.g. approvals, webhooks). */
export function setDaemonUrl(url: string): void {
  daemonUrl = url.replace(/\/+$/, "");
}

/** Get the current daemon base URL. */
export function getDaemonUrl(): string {
  return daemonUrl;
}

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
    timeoutSecs?: number,
    tags?: string[]
  ) =>
    invoke<ExecMultiResult[]>("exec_multi", {
      hosts,
      command,
      force,
      timeoutSecs: timeoutSecs ?? null,
      tags: tags ?? null,
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

  // SSH Keys
  listKeys: () => invoke<SshKeyInfo[]>("list_keys"),
  generateKey: (name: string, comment?: string) =>
    invoke<SshKeyInfo>("generate_key", { name, comment: comment ?? null }),
  importKey: (sourcePath: string, name?: string) =>
    invoke<SshKeyInfo>("import_key", { sourcePath, name: name ?? null }),
  deleteKey: (name: string) => invoke<void>("delete_key", { name }),

  // Playbooks
  listPlaybooks: () => invoke<Playbook[]>("list_playbooks"),
  runPlaybook: (playbook: string, host: string, force: boolean) =>
    invoke<PlaybookRunResult>("run_playbook", { playbook, host, force }),

  // Connection pool
  connectionStatus: () => invoke<ConnectionStatus[]>("connection_status"),
  sshConnect: (host: string) => invoke<void>("ssh_connect", { host }),
  sshDisconnect: (host: string) => invoke<void>("ssh_disconnect", { host }),

  // Webhook config
  getWebhookConfig: () => invoke<WebhookConfig>("get_webhook_config"),
  setWebhookConfig: (config: WebhookConfig) =>
    invoke<void>("set_webhook_config", { config }),

  // Daemon approval polling (Fix-1)
  getDaemonToken: () => invoke<string>("get_daemon_token"),

  // Remote daemons
  listDaemons: () => invoke<DaemonInfo[]>("list_daemons"),

  /** Fetch pending approvals from the running daemon. Returns [] if daemon is unreachable. */
  fetchApprovals: async (): Promise<ApprovalRequest[]> => {
    try {
      const token = await invoke<string>("get_daemon_token");
      const res = await fetch(`${daemonUrl}/approvals`, {
        headers: { Authorization: `Bearer ${token}` },
      });
      if (!res.ok) return [];
      return (await res.json()) as ApprovalRequest[];
    } catch {
      return []; // daemon not running — silent
    }
  },

  /** Approve a pending approval request via the daemon. */
  approvalApprove: async (id: string): Promise<void> => {
    const token = await invoke<string>("get_daemon_token");
    await fetch(`${daemonUrl}/approvals/${id}/approve`, {
      method: "POST",
      headers: { Authorization: `Bearer ${token}` },
    });
  },

  /** Reject a pending approval request via the daemon. */
  approvalReject: async (id: string): Promise<void> => {
    const token = await invoke<string>("get_daemon_token");
    await fetch(`${daemonUrl}/approvals/${id}/reject`, {
      method: "POST",
      headers: { Authorization: `Bearer ${token}` },
    });
  },

  /** Fetch webhook config from the running daemon. */
  fetchWebhookConfig: async (): Promise<WebhookConfig> => {
    const token = await invoke<string>("get_daemon_token");
    const res = await fetch(`${daemonUrl}/webhook/config`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (!res.ok) return { events: ["approval_required"] };
    return (await res.json()) as WebhookConfig;
  },

  /** Save webhook config to the running daemon. */
  saveWebhookConfig: async (config: WebhookConfig): Promise<WebhookConfig> => {
    const token = await invoke<string>("get_daemon_token");
    const res = await fetch(`${daemonUrl}/webhook/config`, {
      method: "PUT",
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(config),
    });
    if (!res.ok) throw new Error(`Failed to save webhook config: ${res.status}`);
    return (await res.json()) as WebhookConfig;
  },
};
