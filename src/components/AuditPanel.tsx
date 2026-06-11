import { History, RefreshCw, SlidersHorizontal } from "lucide-react";
import { useState } from "react";
import { api } from "../api";
import type { AuditEntry, AuditFilter, RiskLevel } from "../types";
import RiskBadge from "./RiskBadge";

type Props = {
  audit: AuditEntry[];
  onRefresh: (filter?: AuditFilter) => void;
};

export default function AuditPanel({ audit, onRefresh }: Props) {
  const [showFilters, setShowFilters] = useState(false);
  const [hostFilter, setHostFilter] = useState("");
  const [riskFilter, setRiskFilter] = useState<RiskLevel | "">("");
  const [limit, setLimit] = useState(50);

  function applyFilters() {
    const filter: AuditFilter = {
      limit,
      host: hostFilter.trim() || null,
      risk_level: riskFilter || null,
    };
    onRefresh(filter);
  }

  function clearFilters() {
    setHostFilter("");
    setRiskFilter("");
    setLimit(50);
    onRefresh();
  }

  return (
    <section className="panel audit-panel">
      <div className="panel-title">
        <History size={16} />
        Audit
        <button
          className="icon-button"
          onClick={() => setShowFilters(!showFilters)}
          title="Toggle filters"
        >
          <SlidersHorizontal size={15} />
        </button>
        <button
          className="icon-button"
          onClick={() => onRefresh()}
          title="Refresh audit"
        >
          <RefreshCw size={15} />
        </button>
      </div>

      {showFilters && (
        <div className="audit-filters">
          <div className="audit-filter-row">
            <label>
              Host
              <input
                value={hostFilter}
                onChange={(e) => setHostFilter(e.target.value)}
                placeholder="all hosts"
              />
            </label>
            <label>
              Risk level
              <select
                value={riskFilter}
                onChange={(e) =>
                  setRiskFilter(e.target.value as RiskLevel | "")
                }
              >
                <option value="">all</option>
                <option value="low">low</option>
                <option value="medium">medium</option>
                <option value="high">high</option>
                <option value="blocked">blocked</option>
              </select>
            </label>
            <label>
              Limit
              <input
                type="number"
                min={1}
                max={500}
                value={limit}
                onChange={(e) => setLimit(Number(e.target.value))}
              />
            </label>
            <div className="audit-filter-actions">
              <button className="primary" onClick={applyFilters}>
                Apply
              </button>
              <button className="secondary" onClick={clearFilters}>
                Clear
              </button>
            </div>
          </div>
        </div>
      )}

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
            <RiskBadge level={entry.risk_level ?? "low"} hideLow />
          </div>
        ))}
        {audit.length === 0 && (
          <div className="empty">No commands executed yet</div>
        )}
      </div>
    </section>
  );
}
