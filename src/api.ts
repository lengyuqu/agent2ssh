import { invoke } from "@tauri-apps/api/core";
import type {
  ApprovalRequest,
  AuditEntry,
  AuditFilter,
  AgentEvent,
  ConnectionStatus,
  DaemonSessionInfo,
  DaemonInfo,
  ExecutionGateStatus,
  ExecMultiResult,
  ExecRequest,
  ExecResult,
  ForwardDirection,
  ForwardRule,
  HostProfile,
  ImportResult,
  PingResult,
  Playbook,
  PlaybookRunResult,
  RiskLevel,
  SessionInfo,
  SftpResult,
  SshKeyInfo,
  TeamConfigExport,
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
  classifyRisk: (command: string, host?: string | null) =>
    host
      ? invoke<RiskLevel>("classify_command_risk_for_host", { command, host })
      : invoke<RiskLevel>("classify_command_risk", { command }),

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
  sessionWrite: (id: string, input: string, force = false) =>
    invoke<void>("session_write", { id, input, force }),
  sessionRead: (id: string, timeoutMs?: number) =>
    invoke<string>("session_read", { id, timeoutMs: timeoutMs ?? 2000 }),
  sessionClose: (id: string) => invoke<void>("session_close", { id }),
  sessionList: () => invoke<SessionInfo[]>("session_list"),
  sessionOpenDaemon: async (host: string): Promise<string> => {
    const token = await invoke<string>("get_daemon_token");
    const res = await fetch(`${daemonUrl}/sessions`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ host, source: "desktop" }),
    });
    if (!res.ok) throw new Error(`Failed to open daemon session: ${res.status}`);
    const body = (await res.json()) as { id: string };
    return body.id;
  },
  sessionWriteDaemon: async (
    id: string,
    input: string,
    force = false
  ): Promise<void> => {
    const token = await invoke<string>("get_daemon_token");
    const res = await fetch(`${daemonUrl}/sessions/${encodeURIComponent(id)}/write`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ input, force, source: "desktop" }),
    });
    if (!res.ok) throw new Error(`Failed to write daemon session: ${res.status}`);
  },
  sessionReadDaemon: async (id: string, timeoutMs?: number): Promise<string> => {
    const token = await invoke<string>("get_daemon_token");
    const params = new URLSearchParams({
      timeout_ms: String(timeoutMs ?? 2000),
      source: "desktop",
    });
    const res = await fetch(
      `${daemonUrl}/sessions/${encodeURIComponent(id)}/read?${params.toString()}`,
      { headers: { Authorization: `Bearer ${token}` } }
    );
    if (!res.ok) throw new Error(`Failed to read daemon session: ${res.status}`);
    const body = (await res.json()) as { output: string };
    return body.output;
  },
  sessionCloseDaemon: async (id: string): Promise<void> => {
    const token = await invoke<string>("get_daemon_token");
    const params = new URLSearchParams({ source: "desktop" });
    const res = await fetch(
      `${daemonUrl}/sessions/${encodeURIComponent(id)}?${params.toString()}`,
      {
        method: "DELETE",
        headers: { Authorization: `Bearer ${token}` },
      }
    );
    if (!res.ok) throw new Error(`Failed to close daemon session: ${res.status}`);
  },
  sessionListDaemon: async (): Promise<DaemonSessionInfo[]> => {
    const token = await invoke<string>("get_daemon_token");
    const res = await fetch(`${daemonUrl}/sessions`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (!res.ok) throw new Error(`Failed to list daemon sessions: ${res.status}`);
    return (await res.json()) as DaemonSessionInfo[];
  },

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

  getGateStatus: async (): Promise<ExecutionGateStatus | null> => {
    try {
      const token = await invoke<string>("get_daemon_token");
      const res = await fetch(`${daemonUrl}/gate`, {
        headers: { Authorization: `Bearer ${token}` },
      });
      if (!res.ok) return null;
      return (await res.json()) as ExecutionGateStatus;
    } catch {
      return null;
    }
  },

  pauseGate: async (reason?: string): Promise<ExecutionGateStatus> => {
    const token = await invoke<string>("get_daemon_token");
    const res = await fetch(`${daemonUrl}/gate/pause`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ source: "desktop", reason: reason ?? null }),
    });
    if (!res.ok) throw new Error(`Failed to pause execution gate: ${res.status}`);
    return (await res.json()) as ExecutionGateStatus;
  },

  resumeGate: async (reason?: string): Promise<ExecutionGateStatus> => {
    const token = await invoke<string>("get_daemon_token");
    const res = await fetch(`${daemonUrl}/gate/resume`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ source: "desktop", reason: reason ?? null }),
    });
    if (!res.ok) throw new Error(`Failed to resume execution gate: ${res.status}`);
    return (await res.json()) as ExecutionGateStatus;
  },

  // Remote daemons
  listDaemons: () => invoke<DaemonInfo[]>("list_daemons"),

  // Team config export/import
  exportTeamConfig: () => invoke<TeamConfigExport>("export_team_config_cmd"),
  importTeamConfig: (config: TeamConfigExport) =>
    invoke<ImportResult>("import_team_config_cmd", { config }),

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

  /** Subscribe to the daemon SSE activity stream using Bearer auth. */
  subscribeEvents: async (
    onEvent: (event: AgentEvent) => void,
    signal?: AbortSignal
  ): Promise<void> => {
    const token = await invoke<string>("get_daemon_token");
    const res = await fetch(`${daemonUrl}/events/stream`, {
      headers: { Authorization: `Bearer ${token}` },
      signal,
    });
    if (!res.ok) throw new Error(`Failed to subscribe to events: ${res.status}`);
    if (!res.body) throw new Error("Event stream has no response body");

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });

      let boundary = buffer.indexOf("\n\n");
      while (boundary >= 0) {
        const frame = buffer.slice(0, boundary);
        buffer = buffer.slice(boundary + 2);
        const data = frame
          .split("\n")
          .filter((line) => line.startsWith("data:"))
          .map((line) => line.slice(5).trimStart())
          .join("\n");
        if (data) {
          onEvent(JSON.parse(data) as AgentEvent);
        }
        boundary = buffer.indexOf("\n\n");
      }
    }
  },
};
