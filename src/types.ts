// =============================================================================
// Frontend type definitions for agent2ssh
//
// These types mirror the Rust structs in src-tauri/src/types.rs,
// src-tauri/src/approval.rs, src-tauri/src/playbook.rs,
// src-tauri/src/notify.rs, src-tauri/src/keys.rs, and
// src-tauri/src/remote.rs.
//
// Type alignment with Rust (src-tauri/src/types.rs et al.):
// - All types mirror Rust structs 1:1 in field names and ordering.
// - RiskLevel: Rust enum (rename_all = "lowercase") → TS union type
//   "low" | "medium" | "high" | "blocked".
// - ApprovalStatus: Rust enum (rename_all = "snake_case") → TS union type.
// - DateTime<Utc> → string (ISO 8601, e.g. "2025-01-01T00:00:00Z").
// - Uuid → string (UUID v4 format).
// - u128 → number (duration_ms, total_duration_ms); safe up to 2^53 in JS.
// - Rust Option<T> maps to T | null in JSON; TypeScript uses optional (?:)
//   with "| null" to handle both missing and null cases.
// - Rust #[serde(default)] fields may be absent from JSON; TypeScript marks
//   these as optional (?).
// - Rust enums with #[serde(rename_all = "lowercase")] serialize as lowercase
//   strings; enums with rename_all = "snake_case" serialize as snake_case.
// - WebhookEvent optional fields use #[serde(skip_serializing_if)] in Rust,
//   so they are absent from JSON (not null) when unset — TS marks them as
//   optional with "| null".
//
// Known differences: NONE — all Rust structs are fully represented.
// Last verified: 2026-06-12
// =============================================================================

// Rust: types.rs — RiskLevel (rename_all = "lowercase")
export type RiskLevel = "low" | "medium" | "high" | "blocked";

export type CloseWindowAction = "minimize_to_tray" | "quit_application";

export type AppPreferences = {
  closeWindowAction: CloseWindowAction;
};

export type RecordingConfig = {
  enabled: boolean;
};

export type RecordingInfo = {
  id: string;
  host: string;
  createdAt: string;
  durationSeconds: number;
  width: number;
  height: number;
  sizeBytes: number;
};

export type RecordingContent = {
  info: RecordingInfo;
  content: string;
};

// Rust: types.rs — HostProfile
// Tags field uses #[serde(default)] so it may be absent (defaults to []).
export type HostFingerprintStatus = {
  host: string;
  address: string;
  hostKeyAlgorithm: string;
  fingerprintSha256: string;
  trusted: boolean;
  expectedHostKeyAlgorithm?: string | null;
  expectedFingerprintSha256?: string | null;
};

export type TrustHostFingerprintRequest = {
  host: string;
  expectedFingerprintSha256?: string | null;
  hostKeyAlgorithm: string;
  fingerprintSha256: string;
};

export type HostProfile = {
  name: string;
  host: string;
  user?: string | null;
  port?: number | null;
  key_path?: string | null;
  password?: string | null;
  passphrase?: string | null;
  jump_host?: string | null;
  proxy_id?: string | null;
  risk_override?: RiskLevel | null;
  tags?: string[];
  group: string;
  env?: string | null;
  role?: string | null;
  owner?: string | null;
};

export type HostGroup = {
  id: string;
  name: string;
};

export type ProxyProtocol = "http" | "socks5";

export type ProxyProfile = {
  id: string;
  name: string;
  protocol: ProxyProtocol;
  host: string;
  port: number;
  username?: string | null;
  password?: string | null;
};

// Rust: types.rs — ExecRequest
// force has #[serde(default)] so it defaults to false when absent.
export type ExecRequest = {
  host: string;
  command: string;
  force?: boolean;
  timeout_secs?: number | null;
  stdin?: string | null;
  max_output_bytes?: number | null;
  reason?: string | null;
  change_id?: string | null;
  source?: string | null;
};

// Rust: types.rs — ExecResult
// duration_ms is u128 in Rust; safe as JS number for realistic values.
// truncated uses #[serde(default)] so it defaults to false when absent.
export type ExecResult = {
  host: string;
  command: string;
  exit_code: number | null;
  stdout: string;
  stderr: string;
  duration_ms: number;
  risk_level: RiskLevel;
  truncated?: boolean;
};

// Rust: types.rs — ExecMultiResult
export type ExecMultiResult = {
  host: string;
  result?: ExecResult | null;
  error?: string | null;
};

// Rust: types.rs — PingResult
export type PingResult = {
  host: string;
  reachable: boolean;
  latency_ms?: number | null;
  error?: string | null;
};

// Daemon GET /health response.
export type DaemonHealth = {
  ok: boolean;
  version?: string | null;
  uptime_secs?: number | null;
  config_dir_available?: boolean | null;
  embedded_ssh_available?: boolean | null;
  embedded_keygen_available?: boolean | null;
  /** @deprecated Use embedded_ssh_available. Kept for older clients. */
  ssh_available?: boolean | null;
  pid?: number | null;
};

export type DaemonControlResult = {
  running: boolean;
  pid?: number | null;
  message: string;
};

export type CliPathStatus = {
  cliDir: string;
  cliPath: string;
  mcpPath: string;
  cliExists: boolean;
  mcpExists: boolean;
  inProcessPath: boolean;
  inUserPath: boolean;
  installed: boolean;
  message: string;
};

export type WebDavSyncConfig = {
  enabled: boolean;
  url: string;
  username?: string | null;
  remotePath: string;
  passwordConfigured: boolean;
};

export type WebDavSyncSaveRequest = {
  enabled: boolean;
  url: string;
  username?: string | null;
  password?: string | null;
  remotePath: string;
};

export type WebDavSyncStatus = {
  configured: boolean;
  enabled: boolean;
  lastAction?: string | null;
  lastSuccess?: boolean | null;
  lastMessage?: string | null;
  lastSyncAt?: string | null;
  lastUploadedBytes?: number | null;
  lastRemotePath?: string | null;
  portableDigest?: string | null;
  syncState: "in_sync" | "local_ahead" | "remote_ahead" | "diverged" | "unknown";
  syncSummary: string;
};

// Rust: webdav_sync.rs — ConfigSnapshotInfo (V4-3)
export type ConfigSnapshotInfo = {
  id: string;
  label?: string | null;
  created_at?: string | null;
  files: string[];
};

export type DiagnosticLogEntry = {
  id: string;
  ts: string;
  level: "trace" | "debug" | "info" | "warn" | "error" | string;
  component: string;
  message: string;
  fields: unknown;
};

export type McpAgentConfigStatus = {
  id: string;
  name: string;
  source: string;
  config_path: string;
  detected: boolean;
  configured: boolean;
  status: string;
  command?: string | null;
  configured_source?: string | null;
  binding_authenticated: boolean;
  recommended_command: string;
};

export type McpAgentConfigureResult = {
  id: string;
  config_path: string;
  backup_path?: string | null;
  command: string;
  source: string;
  message: string;
};

export type McpAgentUninstallResult = {
  id: string;
  config_path: string;
  backup_path?: string | null;
  removed: boolean;
  message: string;
};

// Rust: integrate.rs — AgentSkillStatus (V5 agent skill install/update/uninstall)
export type AgentSkillStatus = {
  dir: string;
  path: string;
  installed: boolean;
  installed_version?: string | null;
  available_version?: string | null;
  update_available: boolean;
};

// Rust: types.rs — SftpResult
// direction is SftpDirection enum (rename_all = "lowercase"): "upload" | "download"
export type SftpResult = {
  host: string;
  local_path: string;
  remote_path: string;
  direction: "upload" | "download";
  duration_ms: number;
  bytes: number;
};

export type SftpExchangeRequest = {
  source_host: string;
  source_path: string;
  destination_host: string;
  destination_path: string;
};

export type SftpExchangeResult = {
  downloaded: SftpResult;
  uploaded: SftpResult;
  local_path: string;
  duration_ms: number;
};

// Rust: tauri_commands.rs — LocalDirEntry / LocalDirListing (local_ls command)
export type LocalDirEntry = {
  name: string;
  is_dir: boolean;
  size: number;
  modified_unix: number | null;
};

export type LocalDirListing = {
  path: string;
  parent: string | null;
  home: string;
  entries: LocalDirEntry[];
};

// Rust: types.rs — WalkEntry (recursive directory walk, J4)
export type WalkEntry = {
  rel_path: string;
  is_dir: boolean;
  size: number;
};

// Rust: session_list_core() returns Vec<(Uuid, String)> which serializes as
// an array of two-element arrays: [id_string, host_string].
export type SessionInfo = [string, string]; // [id, host]

// Daemon GET /sessions returns object rows so the desktop can take over
// sessions opened by MCP, CLI, or another daemon client.
export type DaemonSessionInfo = {
  id: string;
  host: string;
};

export type TerminalBroadcastTarget = {
  terminal_id: string;
  host: string;
};

export type TerminalBroadcastRequest = {
  targets: TerminalBroadcastTarget[];
  command: string;
  force?: boolean;
  all_or_nothing?: boolean;
};

export type TerminalBroadcastTargetResult = TerminalBroadcastTarget & {
  risk_level: RiskLevel;
  requires_approval: boolean;
  matched_policy: string | null;
  authorized: boolean;
  approval_granted: boolean;
  sent: boolean;
  error: string | null;
};

export type TerminalBroadcastResponse = {
  broadcast_id: string;
  enqueued_any: boolean;
  all_or_nothing: boolean;
  targets: TerminalBroadcastTargetResult[];
};

// Rust: types.rs — ForwardDirection (rename_all = "lowercase")
export type ForwardDirection = "local" | "remote";

// Rust: types.rs — ForwardRule
// id is uuid::Uuid in Rust, serialized as a UUID-format string.
export type ForwardRule = {
  id: string;
  host: string;
  direction: ForwardDirection;
  bind_port: number;
  target_host: string;
  target_port: number;
};

// Rust: types.rs — AuditFilter
// Used as query parameters in daemon GET /audit endpoint.
export type AuditFilter = {
  host?: string | null;
  risk_level?: RiskLevel | null;
  exit_code?: number | null;
  since?: string | null;
  until?: string | null;
  limit?: number;
};

// Rust: types.rs — AuditEntry
// id is uuid::Uuid; ts is DateTime<Utc> (ISO-8601 string).
export type AuditEntry = {
  id: string;
  ts: string;
  host: string;
  command: string;
  exit_code: number | null;
  duration_ms: number;
  risk_level: RiskLevel;
  reason?: string | null;
  change_id?: string | null;
  source?: string | null;
};

export type AgentEventType =
  | "stream_connected"
  | "exec_started"
  | "exec_output"
  | "exec_completed"
  | "approval_requested"
  | "approval_responded"
  | "host_connected"
  | "host_disconnected"
  | "session_opened"
  | "session_input"
  | "session_output"
  | "session_closed"
  | "audit_rotated"
  | "config_changed"
  | "gate_changed"
  | "gate_rejected"
  | "limit_rejected"
  | "anomaly_detected";

export type AgentEvent = {
  id: string;
  event_type: AgentEventType;
  timestamp: string;
  data: Record<string, unknown>;
};

export type ExecutionGateMode = "active" | "paused";

export type ExecutionGateStatus = {
  mode: ExecutionGateMode;
  updated_at?: string | null;
  updated_by?: string | null;
  reason?: string | null;
};

// Rust: approval.rs — ApprovalStatus (rename_all = "snake_case")
export type ApprovalStatus = "pending" | "approved" | "rejected" | "timed_out";

// Rust: approval.rs — ApprovalRequest
// id is uuid::Uuid; requested_at is DateTime<Utc>.
export type ApprovalRequest = {
  id: string;
  host: string;
  command: string;
  risk_level: RiskLevel;
  requested_at: string;
  ttl_secs: number;
  status: ApprovalStatus;
};

// Rust: keys.rs — SshKeyInfo
// created_at is Option<String> in Rust (not a DateTime; stored as display string).
export type SshKeyInfo = {
  name: string;
  private_path: string;
  public_path: string;
  public_key: string;
  key_type: string;
  created_at?: string | null;
};

// Rust: types.rs — ConnectionStatus
export type ConnectionStatus = {
  host: string;
  connected: boolean;
  socket_path?: string | null;
  // K5: liveness/reconnect state from the connection supervisor.
  healthy?: boolean;
  reconnecting?: boolean;
  last_error?: string | null;
};

// Rust: playbook.rs — Playbook
// tags and risk_override use #[serde(default)].
export type Playbook = {
  name: string;
  description: string;
  steps: string[];
  tags: string[];
  risk_override?: RiskLevel | null;
  advanced_steps?: PlaybookStepDefinition[] | null;
};

export type PlaybookParam = {
  name: string;
  description?: string | null;
  default?: string | null;
  required: boolean;
};

export type PlaybookStepDefinition = {
  command: string;
  description?: string | null;
  params: PlaybookParam[];
};

// Rust: playbook.rs — PlaybookStepResult
export type PlaybookStepResult = {
  step: number;
  command: string;
  result?: ExecResult | null;
  error?: string | null;
};

// Rust: playbook.rs — PlaybookRunResult
// total_duration_ms is u128 in Rust; safe as JS number for realistic values.
export type PlaybookRunResult = {
  playbook: string;
  host: string;
  steps_completed: PlaybookStepResult[];
  success: boolean;
  total_duration_ms: number;
};

// Rust: notify.rs — WebhookConfig
// Default events in Rust: ["approval_required"]. The events field always has a
// value due to #[serde(default = "default_events")] on the Rust side.
export type WebhookConfig = {
  url?: string | null;
  events: string[];
  secret?: string | null;
};

// Rust: notify.rs — WebhookEvent
// Optional fields use #[serde(skip_serializing_if = "Option::is_none")] in Rust,
// so they will be absent from JSON (not null) when not set.
export type WebhookEvent = {
  event: string;
  host: string;
  command: string;
  approval_id?: string | null;
  risk_level?: string | null;
  exit_code?: number | null;
};

// Rust: remote.rs — DaemonInfo
// "localhost" is always the first entry when returned from list_daemons_core().
export type DaemonInfo = {
  alias: string;
  url: string;
  connected: boolean;
};

// Rust: core.rs — TeamConfigExport
// Host profiles with key_path stripped, plus optional raw TOML content.
export type TeamConfigExport = {
  hosts: HostProfile[];
  risk_rules?: string | null;
  playbooks?: string | null;
};

// Rust: core.rs — ImportResult
// Result of importing a team configuration.
export type ImportResult = {
  hosts_added: number;
  hosts_skipped: number;
  hosts_updated: number;
  risk_rules_imported: boolean;
  playbooks_imported: boolean;
};

export type ForwardRuleStats = {
  bytes_tx: number;
  bytes_rx: number;
  connections: number;
  state: "running" | "stopped" | "error";
};

// Rust: types.rs — HighlightRule
// B24: Terminal highlight rule for regex-based output decoration.
export type HighlightRule = {
  keyword: string;
  name: string;
  color: string;
  enabled: boolean;
  is_regex: boolean;
  is_case_sensitive: boolean;
};

// Rust: snippets.rs — Snippet
// description is omitted when the backend value is None.
export type Snippet = {
  name: string;
  command: string;
  description?: string | null;
};
