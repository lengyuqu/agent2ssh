import { History } from "lucide-react";
import type { AuditEntry, RiskLevel } from "../types";

type Props = {
  audit: AuditEntry[];
};

function RiskBadge({ level }: { level: RiskLevel }) {
  if (level === "low") return null;
  const map: Record<RiskLevel, string> = {
    low:     "risk-low",
    medium:  "risk-medium",
    high:    "risk-high",
    blocked: "risk-blocked",
  };
  return <span className={`risk-badge ${map[level]}`}>{level}</span>;
}

export default function AuditPanel({ audit }: Props) {
  return (
    <section className="panel audit-panel">
      <div className="panel-title">
        <History size={16} />
        Audit
      </div>
      <div className="audit-list">
        {audit.map((entry) => (
          <div
            className={`audit-row ${entry.risk_level === "high" ? "audit-row--high" : ""}`}
            key={entry.id}
          >
            <span>{new Date(entry.ts).toLocaleString()}</span>
            <strong>{entry.host}</strong>
            <code>{entry.command}</code>
            <em>
              exit={entry.exit_code ?? "signal"} {entry.duration_ms}ms
            </em>
            <RiskBadge level={entry.risk_level ?? "low"} />
          </div>
        ))}
        {audit.length === 0 && (
          <div className="empty">No commands executed yet</div>
        )}
      </div>
    </section>
  );
}
