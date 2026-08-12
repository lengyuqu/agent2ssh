import { invoke } from "@tauri-apps/api/core";
import type {
  ApprovalRequest,
  AuditEntry,
  AuditFilter,
  AgentEvent,
  AppPreferences,
  CliPathStatus,
  ConfigSnapshotInfo,
  ConnectionStatus,
  DaemonControlResult,
  DaemonHealth,
  DaemonSessionInfo,
  DaemonInfo,
  DiagnosticLogEntry,
  ExecutionGateStatus,
  ExecMultiResult,
  ExecRequest,
  ExecResult,
  ForwardDirection,
  ForwardRule,
  HighlightRule,
  HostFingerprintStatus,
  HostGroup,
  HostProfile,
  ImportResult,
  LocalDirListing,
  WalkEntry,
  AgentSkillStatus,
  McpAgentConfigureResult,
  McpAgentConfigStatus,
  McpAgentUninstallResult,
  PingResult,
  Playbook,
  PlaybookRunResult,
  ProxyProfile,
  RiskLevel,
  SftpExchangeRequest,
  SftpExchangeResult,
  SessionInfo,
  SftpResult,
  SshKeyInfo,
  TeamConfigExport,
  TrustHostFingerprintRequest,
  WebDavSyncConfig,
  WebDavSyncSaveRequest,
  WebDavSyncStatus,
  WebhookConfig,
} from "./types";

let daemonUrl = "http://127.0.0.1:7722";

/** Header used to propagate a correlation id to the daemon (mirrors the Rust `TRACE_ID_HEADER`). */
const TRACE_ID_HEADER = "X-Agent2SSH-Trace-Id";

/**
 * Stable per-session correlation id. Tagged onto every frontend diagnostic and
 * sent to the daemon on direct HTTP calls, so a desktop session's frontend logs
 * and the daemon log lines it triggers can be correlated.
 */
const SESSION_TRACE_ID: string =
  typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `sess-${Date.now()}-${Math.random().toString(16).slice(2)}`;

/** Get the current desktop session's correlation id. */
export function getSessionTraceId(): string {
  return SESSION_TRACE_ID;
}

/** Headers for direct daemon fetches that carry the session correlation id. */
export function traceHeaders(): Record<string, string> {
  return { [TRACE_ID_HEADER]: SESSION_TRACE_ID };
}

export function logDiagnostic(
  level: "debug" | "info" | "warn" | "error",
  component: string,
  message: string,
  fields?: Record<string, unknown>
): void {
  invoke("write_diagnostic_log", {
    level,
    component,
    message,
    // Stamp the session trace id unless the caller already supplied one.
    fields: { trace_id: SESSION_TRACE_ID, ...(fields ?? {}) },
  }).catch(() => {
    // Diagnostics must never break the user workflow.
  });
}

/**
 * Fire-and-forget error reporter for use inside `catch` blocks across the UI.
 * Normalizes the caught value (Error → message + stack) and forwards it to the
 * backend diagnostic log at `error` level. Never throws and never awaits, so it
 * is safe to call alongside `setError(...)` without affecting the user flow.
 */
export function reportError(
  component: string,
  message: string,
  err: unknown,
  fields?: Record<string, unknown>
): void {
  const detail =
    err instanceof Error
      ? { error: err.message, stack: err.stack }
      : { error: String(err) };
  logDiagnostic("error", component, message, { ...detail, ...(fields ?? {}) });
}

/** Change the base URL used for direct daemon HTTP calls (e.g. approvals, webhooks). */
export function setDaemonUrl(url: string): void {
  daemonUrl = url.replace(/\/+$/, "");
}

/** Get the current daemon base URL. */
export function getDaemonUrl(): string {
  return daemonUrl;
}

function objectValue(value: unknown, context: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${context} returned an invalid response`);
  }
  return value as Record<string, unknown>;
}

function stringField(value: unknown, field: string, context: string): string {
  if (typeof value !== "string") {
    throw new Error(`${context} response missing string field '${field}'`);
  }
  return value;
}

export const api = {
  // Host management
  listHosts: () => invoke<HostProfile[]>("list_hosts"),
  addHost: (host: HostProfile) => invoke<HostProfile>("add_host", { host }),
  updateHost: (originalName: string, host: HostProfile) =>
    invoke<HostProfile>("update_host", { originalName, host }),
  removeHost: (name: string) => invoke<void>("remove_host", { name }),
  listHostGroups: () => invoke<HostGroup[]>("list_host_groups"),
  saveHostGroup: (group: HostGroup) => invoke<HostGroup>("save_host_group", { group }),
  deleteHostGroup: (id: string) => invoke<boolean>("delete_host_group", { id }),
  listProxies: () => invoke<ProxyProfile[]>("list_proxies"),
  saveProxy: (proxy: ProxyProfile) => invoke<ProxyProfile>("save_proxy", { proxy }),
  deleteProxy: (id: string) => invoke<boolean>("delete_proxy", { id }),
  importSshConfig: (path?: string) =>
    invoke<HostProfile[]>("import_ssh_config", { path: path ?? null }),

  // WebDAV sync
  getWebDavSyncConfig: () => invoke<WebDavSyncConfig>("get_webdav_sync_config"),
  setWebDavSyncConfig: (config: WebDavSyncSaveRequest) =>
    invoke<WebDavSyncConfig>("set_webdav_sync_config", { config }),
  getWebDavSyncStatus: () => invoke<WebDavSyncStatus>("get_webdav_sync_status"),
  testWebDavSync: () => invoke<WebDavSyncStatus>("test_webdav_sync"),
  pushWebDavSync: () => invoke<WebDavSyncStatus>("push_webdav_sync"),

  // V4-3: config snapshots + templates (independent of WebDAV sync above —
  // reuses the same backup-directory mechanism, but works with no WebDAV
  // configured at all).
  listConfigSnapshots: () => invoke<ConfigSnapshotInfo[]>("list_config_snapshots_cmd"),
  createConfigSnapshot: (label: string) =>
    invoke<ConfigSnapshotInfo>("create_config_snapshot", { label }),
  restoreConfigSnapshot: (id: string) =>
    invoke<ConfigSnapshotInfo>("restore_config_snapshot_cmd", { id }),
  deleteConfigSnapshot: (id: string) => invoke<void>("delete_config_snapshot_cmd", { id }),
  applyConfigTemplate: (files: Array<[string, string]>) =>
    invoke<ConfigSnapshotInfo>("apply_config_template_cmd", { files }),

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
  sftpUpload: (
    host: string,
    localPath: string,
    remotePath: string,
    opts?: { resume?: boolean; transferId?: string }
  ) =>
    invoke<SftpResult>("sftp_upload", {
      request: {
        host,
        local_path: localPath,
        remote_path: remotePath,
        resume: opts?.resume ?? false,
        transfer_id: opts?.transferId ?? null,
      },
    }),
  // K6: cancel an in-flight transfer by its id.
  sftpCancel: (transferId: string) =>
    invoke<boolean>("sftp_cancel", { transferId }),
  sftpExchange: (
    sourceHost: string,
    sourcePath: string,
    destinationHost: string,
    destinationPath: string
  ) =>
    invoke<SftpExchangeResult>("sftp_exchange", {
      request: {
        source_host: sourceHost,
        source_path: sourcePath,
        destination_host: destinationHost,
        destination_path: destinationPath,
      } as SftpExchangeRequest,
    }),
  sftpDownload: (
    host: string,
    remotePath: string,
    localPath: string,
    opts?: { resume?: boolean; transferId?: string }
  ) =>
    invoke<SftpResult>("sftp_download", {
      request: {
        host,
        remote_path: remotePath,
        local_path: localPath,
        resume: opts?.resume ?? false,
        transfer_id: opts?.transferId ?? null,
      },
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
  /** V3-1: SFTP panel text preview. Throws for files over ~1MB or that aren't
   * valid UTF-8 — callers fall back to a metadata card in that case. */
  sftpReadText: (host: string, path: string, timeoutSecs?: number) =>
    invoke<ExecResult>("sftp_read_text", {
      host,
      path,
      timeoutSecs: timeoutSecs ?? null,
    }),
  localLs: (path?: string | null) =>
    invoke<LocalDirListing>("local_ls", { path: path ?? null }),
  localWalk: (root: string) => invoke<WalkEntry[]>("local_walk", { root }),
  localMkdir: (path: string) => invoke<void>("local_mkdir", { path }),
  /** V3-1: local counterpart of sftpReadText for the "This computer" side. */
  localReadText: (path: string) => invoke<string>("local_read_text", { path }),
  sftpWalk: (host: string, root: string) =>
    invoke<WalkEntry[]>("sftp_walk", { host, root }),

  // Sessions
  sessionOpen: (host: string) => invoke<string>("session_open", { host }),
  sessionWrite: (id: string, input: string, force = false) =>
    invoke<void>("session_write", { id, input, force }),
  sessionRead: (id: string, timeoutMs?: number) =>
    invoke<string>("session_read", { id, timeoutMs: timeoutMs ?? 2000 }),
  sessionClose: (id: string) => invoke<void>("session_close", { id }),
  sessionList: () => invoke<SessionInfo[]>("session_list"),

  // B24: Terminal Highlight rules
  listHighlights: () => invoke<HighlightRule[]>("list_highlights"),
  addHighlight: (rule: HighlightRule) => invoke<HighlightRule[]>("add_highlight", { rule }),
  removeHighlight: (keyword: string) => invoke<HighlightRule[]>("remove_highlight", { keyword }),
  updateHighlight: (oldKeyword: string, rule: HighlightRule) =>
    invoke<HighlightRule[]>("update_highlight", { oldKeyword, rule }),
  resetHighlights: () => invoke<HighlightRule[]>("reset_highlights"),
  sessionOpenDaemon: async (host: string): Promise<string> => {
    const token = await invoke<string>("get_daemon_token");
    const res = await fetch(`${daemonUrl}/sessions`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${token}`, [TRACE_ID_HEADER]: SESSION_TRACE_ID,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ host, source: "desktop" }),
    });
    if (!res.ok) throw new Error(`Failed to open daemon session: ${res.status}`);
    const body = objectValue(await res.json(), "Open daemon session");
    return stringField(body.id, "id", "Open daemon session");
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
        Authorization: `Bearer ${token}`, [TRACE_ID_HEADER]: SESSION_TRACE_ID,
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
      { headers: { Authorization: `Bearer ${token}`, [TRACE_ID_HEADER]: SESSION_TRACE_ID } }
    );
    if (!res.ok) throw new Error(`Failed to read daemon session: ${res.status}`);
    const body = objectValue(await res.json(), "Read daemon session");
    return stringField(body.output, "output", "Read daemon session");
  },
  sessionCloseDaemon: async (id: string): Promise<void> => {
    const token = await invoke<string>("get_daemon_token");
    const params = new URLSearchParams({ source: "desktop" });
    const res = await fetch(
      `${daemonUrl}/sessions/${encodeURIComponent(id)}?${params.toString()}`,
      {
        method: "DELETE",
        headers: { Authorization: `Bearer ${token}`, [TRACE_ID_HEADER]: SESSION_TRACE_ID },
      }
    );
    if (!res.ok) throw new Error(`Failed to close daemon session: ${res.status}`);
  },
  sessionListDaemon: async (): Promise<DaemonSessionInfo[]> => {
    const token = await invoke<string>("get_daemon_token");
    const res = await fetch(`${daemonUrl}/sessions`, {
      headers: { Authorization: `Bearer ${token}`, [TRACE_ID_HEADER]: SESSION_TRACE_ID },
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

  // Diagnostics
  listDiagnosticLogs: (limit?: number) =>
    invoke<DiagnosticLogEntry[]>("list_diagnostic_logs", { limit: limit ?? null }),
  writeDiagnosticLog: (
    level: "debug" | "info" | "warn" | "error",
    component: string,
    message: string,
    fields?: Record<string, unknown>
  ) =>
    invoke<DiagnosticLogEntry>("write_diagnostic_log", {
      level,
      component,
      message,
      fields: fields ?? null,
    }),
  clearDiagnosticLogs: () => invoke<void>("clear_diagnostic_logs"),
  exportDiagnosticBundle: () => invoke<string>("export_diagnostic_bundle"),

  // K10: opt-in, local-only telemetry toggle.
  getTelemetryEnabled: () => invoke<boolean>("get_telemetry_enabled"),
  setTelemetryEnabled: (enabled: boolean) =>
    invoke<void>("set_telemetry_enabled", { enabled }),

  // K1: app-managed credential store (master password).
  secretsStatus: () => invoke<{ initialized: boolean; unlocked: boolean }>("secrets_status"),
  secretsUnlock: (password: string) => invoke<void>("secrets_unlock", { password }),
  secretsChangePassword: (newPassword: string) =>
    invoke<void>("secrets_change_password", { newPassword }),

  // SSH Keys
  listKeys: () => invoke<SshKeyInfo[]>("list_keys"),
  generateKey: (name: string, comment?: string) =>
    invoke<SshKeyInfo>("generate_key", { name, comment: comment ?? null }),
  importKey: (sourcePath: string, name?: string) =>
    invoke<SshKeyInfo>("import_key", { sourcePath, name: name ?? null }),
  deleteKey: (name: string) => invoke<void>("delete_key", { name }),

  // Playbooks
  listPlaybooks: () => invoke<Playbook[]>("list_playbooks"),
  savePlaybook: (playbook: Playbook) =>
    invoke<Playbook>("save_playbook", { playbook }),
  deletePlaybook: (name: string) => invoke<boolean>("delete_playbook", { name }),
  runPlaybook: (playbook: string, host: string, force: boolean) =>
    invoke<PlaybookRunResult>("run_playbook", { playbook, host, force }),

  // Connection pool
  connectionStatus: () => invoke<ConnectionStatus[]>("connection_status"),
  sshConnect: (host: string) => invoke<void>("ssh_connect", { host }),
  sshDisconnect: (host: string) => invoke<void>("ssh_disconnect", { host }),
  getHostFingerprintStatus: (host: string) =>
    invoke<HostFingerprintStatus>("get_host_fingerprint_status", { host }),
  trustHostFingerprint: (request: TrustHostFingerprintRequest) =>
    invoke<void>("trust_host_fingerprint", { request }),

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
        headers: { Authorization: `Bearer ${token}`, [TRACE_ID_HEADER]: SESSION_TRACE_ID },
      });
      if (!res.ok) {
        logDiagnostic("warn", "frontend", "gate status request returned non-OK", {
          status: res.status,
        });
        return null;
      }
      return (await res.json()) as ExecutionGateStatus;
    } catch (err) {
      logDiagnostic("warn", "frontend", "gate status request failed", {
        error: String(err),
      });
      return null;
    }
  },

  getDaemonHealth: async (): Promise<DaemonHealth | null> => {
    try {
      const res = await fetch(`${daemonUrl}/health`, { headers: traceHeaders() });
      if (!res.ok) {
        logDiagnostic("warn", "frontend", "daemon health request returned non-OK", {
          status: res.status,
        });
        return null;
      }
      return (await res.json()) as DaemonHealth;
    } catch (err) {
      logDiagnostic("warn", "frontend", "daemon health request failed", {
        error: String(err),
      });
      return null;
    }
  },

  daemonStatus: () => invoke<DaemonControlResult>("daemon_status"),
  daemonStart: () => invoke<DaemonControlResult>("daemon_start"),
  daemonStop: () => invoke<DaemonControlResult>("daemon_stop"),
  daemonRestart: () => invoke<DaemonControlResult>("daemon_restart"),
  quitApp: () => invoke<void>("quit_app"),
  getAppPreferences: () => invoke<AppPreferences>("get_app_preferences"),
  setAppPreferences: (preferences: AppPreferences) =>
    invoke<AppPreferences>("set_app_preferences", { preferences }),
  getCliPathStatus: () => invoke<CliPathStatus>("get_cli_path_status"),
  installCliToPath: () => invoke<CliPathStatus>("install_cli_to_path"),
  removeCliFromPath: () => invoke<CliPathStatus>("remove_cli_from_path"),
  setTrayLabels: (params: { openLabel: string; quitLabel: string; tooltip?: string | null }) =>
    invoke<void>("set_tray_labels", {
      openLabel: params.openLabel,
      quitLabel: params.quitLabel,
      tooltip: params.tooltip ?? null,
    }),
  listMcpAgentConfigs: () => invoke<McpAgentConfigStatus[]>("list_mcp_agent_configs"),
  configureMcpAgent: (agentId: string) =>
    invoke<McpAgentConfigureResult>("configure_mcp_agent", { agentId }),
  uninstallMcpAgent: (agentId: string) =>
    invoke<McpAgentUninstallResult>("uninstall_mcp_agent", { agentId }),
  agentSkillStatus: () => invoke<AgentSkillStatus>("agent_skill_status"),
  installAgentSkill: () => invoke<AgentSkillStatus>("install_agent_skill"),
  uninstallAgentSkill: () => invoke<AgentSkillStatus>("uninstall_agent_skill"),

  pauseGate: async (reason?: string): Promise<ExecutionGateStatus> => {
    const token = await invoke<string>("get_daemon_token");
    const res = await fetch(`${daemonUrl}/gate/pause`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${token}`, [TRACE_ID_HEADER]: SESSION_TRACE_ID,
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
        Authorization: `Bearer ${token}`, [TRACE_ID_HEADER]: SESSION_TRACE_ID,
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
        headers: { Authorization: `Bearer ${token}`, [TRACE_ID_HEADER]: SESSION_TRACE_ID },
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
    const res = await fetch(`${daemonUrl}/approvals/${id}/approve`, {
      method: "POST",
      headers: { Authorization: `Bearer ${token}`, [TRACE_ID_HEADER]: SESSION_TRACE_ID },
    });
    if (!res.ok) throw new Error(`Failed to approve request: ${res.status}`);
  },

  /** Reject a pending approval request via the daemon. */
  approvalReject: async (id: string): Promise<void> => {
    const token = await invoke<string>("get_daemon_token");
    const res = await fetch(`${daemonUrl}/approvals/${id}/reject`, {
      method: "POST",
      headers: { Authorization: `Bearer ${token}`, [TRACE_ID_HEADER]: SESSION_TRACE_ID },
    });
    if (!res.ok) throw new Error(`Failed to reject request: ${res.status}`);
  },

  /** Fetch webhook config from the running daemon. */
  fetchWebhookConfig: async (): Promise<WebhookConfig> => {
    const token = await invoke<string>("get_daemon_token");
    const res = await fetch(`${daemonUrl}/webhook/config`, {
      headers: { Authorization: `Bearer ${token}`, [TRACE_ID_HEADER]: SESSION_TRACE_ID },
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
        Authorization: `Bearer ${token}`, [TRACE_ID_HEADER]: SESSION_TRACE_ID,
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
    signal?: AbortSignal,
    onOpen?: () => void
  ): Promise<void> => {
    const token = await invoke<string>("get_daemon_token");
    logDiagnostic("debug", "frontend", "opening daemon event stream", { daemonUrl });
    const res = await fetch(`${daemonUrl}/events/stream`, {
      headers: { Authorization: `Bearer ${token}`, [TRACE_ID_HEADER]: SESSION_TRACE_ID },
      signal,
    });
    if (!res.ok) throw new Error(`Failed to subscribe to events: ${res.status}`);
    if (!res.body) throw new Error("Event stream has no response body");
    logDiagnostic("info", "frontend", "daemon event stream connected", { daemonUrl });
    onOpen?.();

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";

    while (true) {
      const { done, value } = await reader.read();
      if (done) {
        logDiagnostic("warn", "frontend", "daemon event stream closed by reader", { daemonUrl });
        break;
      }
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
          try {
            onEvent(JSON.parse(data) as AgentEvent);
          } catch (err) {
            reportError("frontend", "failed to parse daemon event frame", err, {
              preview: data.slice(0, 512),
            });
          }
        }
        boundary = buffer.indexOf("\n\n");
      }
    }
  },
};
