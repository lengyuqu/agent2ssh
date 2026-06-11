export type RiskLevel = "low" | "medium" | "high" | "blocked";

export type HostProfile = {
  name: string;
  host: string;
  user?: string | null;
  port?: number | null;
  key_path?: string | null;
};

export type ExecResult = {
  host: string;
  command: string;
  exit_code: number | null;
  stdout: string;
  stderr: string;
  duration_ms: number;
  risk_level: RiskLevel;
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
