import { History } from "lucide-react";
import type { AuditEntry } from "../types";

type Props = {
  audit: AuditEntry[];
};

export default function AuditPanel({ audit }: Props) {
  return (
    <section className="panel audit-panel">
      <div className="panel-title">
        <History size={16} />
        Audit
      </div>
      <div className="audit-list">
        {audit.map((entry) => (
          <div className="audit-row" key={entry.id}>
            <span>{new Date(entry.ts).toLocaleString()}</span>
            <strong>{entry.host}</strong>
            <code>{entry.command}</code>
            <em>
              exit={entry.exit_code ?? "signal"} {entry.duration_ms}ms
            </em>
          </div>
        ))}
        {audit.length === 0 && (
          <div className="empty">No commands executed yet</div>
        )}
      </div>
    </section>
  );
}
