export type RiskLevel = "low" | "medium" | "high" | "blocked";

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

export type ExecRequest = {
  host: string;
  command: string;
  force?: boolean;
  timeout_secs?: number | null;
  stdin?: string | null;
  max_output_bytes?: number | null;
};

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

export type ExecMultiResult = {
  host: string;
  result?: ExecResult | null;
  error?: string | null;
};

export type PingResult = {
  host: string;
  reachable: boolean;
  latency_ms?: number | null;
  error?: string | null;
};

export type SftpResult = {
  host: string;
  local_path: string;
  remote_path: string;
  direction: "upload" | "download";
  duration_ms: number;
};

export type SessionInfo = [string, string]; // [id, host]

export type ForwardDirection = "local" | "remote";

export type ForwardRule = {
  id: string;
  host: string;
  direction: ForwardDirection;
  bind_port: number;
  target_host: string;
  target_port: number;
};

export type AuditFilter = {
  host?: string | null;
  risk_level?: RiskLevel | null;
  exit_code?: number | null;
  since?: string | null;
  until?: string | null;
  limit?: number;
};

export type AuditEntry = {
  id: string;
  ts: string;
  host: string;
  command: string;
  exit_code: number | null;
  duration_ms: number;
  risk_level: RiskLevel;
};

export type ApprovalStatus = "pending" | "approved" | "rejected" | "timed_out";

export type ApprovalRequest = {
  id: string;
  host: string;
  command: string;
  risk_level: RiskLevel;
  requested_at: string;
  ttl_secs: number;
  status: ApprovalStatus;
};

export type SshKeyInfo = {
  name: string;
  private_path: string;
  public_path: string;
  public_key: string;
  key_type: string;
  created_at?: string | null;
};

export type ConnectionStatus = {
  host: string;
  connected: boolean;
  socket_path?: string | null;
};

export type Playbook = {
  name: string;
  description: string;
  steps: string[];
  tags: string[];
  risk_override?: RiskLevel | null;
};

export type PlaybookStepResult = {
  step: number;
  command: string;
  result?: ExecResult | null;
  error?: string | null;
};

export type PlaybookRunResult = {
  playbook: string;
  host: string;
  steps_completed: PlaybookStepResult[];
  success: boolean;
  total_duration_ms: number;
};

export type WebhookConfig = {
  url?: string | null;
  events: string[];
  secret?: string | null;
};

export type WebhookEvent = {
  event: string;
  host: string;
  command: string;
  approval_id?: string | null;
  risk_level?: string | null;
  exit_code?: number | null;
};

export type DaemonInfo = {
  alias: string;
  url: string;
  connected: boolean;
};
