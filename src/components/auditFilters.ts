import type { AuditEntry, RiskLevel } from "../types";

export type AuditRange = "24h" | "7d" | "30d";

export const AUDIT_RANGE_MS: Record<AuditRange, number> = {
  "24h": 24 * 60 * 60 * 1000,
  "7d": 7 * 24 * 60 * 60 * 1000,
  "30d": 30 * 24 * 60 * 60 * 1000,
};

/** Empty `source` is grouped under this key so the chart and the table agree. */
export const AUDIT_UNKNOWN_SOURCE = "unknown";

export function auditSourceKey(entry: AuditEntry): string {
  const value = entry.source?.trim();
  return value ? value : AUDIT_UNKNOWN_SOURCE;
}

export function auditEntryInRange(entry: AuditEntry, range: AuditRange, now = Date.now()): boolean {
  const ts = new Date(entry.ts).getTime();
  return Number.isFinite(ts) && ts >= now - AUDIT_RANGE_MS[range];
}

export function auditEntryMatchesHost(entry: AuditEntry, host: string): boolean {
  const query = host.trim().toLowerCase();
  if (!query) return true;
  return entry.host.toLowerCase().includes(query);
}

export function auditEntryMatchesSource(entry: AuditEntry, source: string): boolean {
  if (!source) return true;
  return auditSourceKey(entry) === source;
}

export function auditEntryMatchesRisk(entry: AuditEntry, risk: RiskLevel | ""): boolean {
  if (!risk) return true;
  return entry.risk_level === risk;
}

export function auditEntryMatchesSearch(entry: AuditEntry, search: string): boolean {
  const query = search.trim().toLowerCase();
  if (!query) return true;
  return [entry.command, entry.original_command, entry.host, entry.source, entry.reason, entry.change_id]
    .filter((part) => part)
    .join("\n")
    .toLowerCase()
    .includes(query);
}
