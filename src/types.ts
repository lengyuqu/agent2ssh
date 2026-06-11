export type RiskLevel = "low" | "medium" | "high" | "blocked";

export type HostProfile = {
  name: string;
  host: string;
  user?: string | null;
  port?: number | null;
  key_path?: string | null;
  jump_host?: string | null;
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
