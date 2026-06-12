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

// Rust: types.rs — HostProfile
// Tags field uses #[serde(default)] so it may be absent (defaults to []).
export type HostProfile = {
  name: string;
  host: string;
  user?: string | null;
  port?: number | null;
  key_path?: string | null;
  jump_host?: string | null;
  risk_override?: RiskLevel | null;
  tags?: string[];
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

// Rust: types.rs — SftpResult
// direction is SftpDirection enum (rename_all = "lowercase"): "upload" | "download"
export type SftpResult = {
  host: string;
  local_path: string;
  remote_path: string;
  direction: "upload" | "download";
  duration_ms: number;
};

// Rust: session_list_core() returns Vec<(Uuid, String)> which serializes as
// an array of two-element arrays: [id_string, host_string].
export type SessionInfo = [string, string]; // [id, host]

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
};

// Rust: playbook.rs — Playbook
// tags and risk_override use #[serde(default)].
export type Playbook = {
  name: string;
  description: string;
  steps: string[];
  tags: string[];
  risk_override?: RiskLevel | null;
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
