import { Activity, ChevronDown, ChevronRight, RefreshCw, Search, ShieldAlert } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import type { AgentEvent, AuditEntry } from "../types";

const MAX_EVENTS = 80;
const AUDIT_POLL_MS = 3000;

type ActivityItem = {
  id: string;
  ts: string;
  source: string;
  kind: string;
  host?: string;
  command?: string;
  detail?: string;
  exitCode?: number | null;
  riskLevel?: string;
  changeId?: string | null;
  sessionId?: string;
  raw?: Record<string, unknown>;
};

type Props = {
  audit: AuditEntry[];
};

function asString(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function asNumber(value: unknown): number | null | undefined {
  return typeof value === "number" ? value : value === null ? null : undefined;
}

function eventToItem(event: AgentEvent): ActivityItem {
  const data = event.data;
  const output = asString(data.output_preview);
  const input = asString(data.input_preview);
  const stream = asString(data.stream);
  const sessionId = asString(data.session_id);
  const source = asString(data.source) ?? "daemon";

  let detail = output ?? input;
  if (!detail && sessionId) detail = `session ${sessionId.slice(0, 8)}`;
  if (stream && detail) detail = `${stream}: ${detail}`;

  return {
    id: event.id,
    ts: event.timestamp,
    source,
    kind: event.event_type.split("_").join(" "),
    host: asString(data.host),
    command: asString(data.command),
    detail,
    exitCode: asNumber(data.exit_code),
    riskLevel: asString(data.risk_level),
    changeId: asString(data.change_id) ?? null,
    sessionId,
    raw: data,
  };
}

function auditToItem(entry: AuditEntry): ActivityItem {
  return {
    id: `audit-${entry.id}`,
    ts: entry.ts,
    source: entry.source ?? "audit",
    kind: "exec recorded",
    host: entry.host,
    command: entry.command,
    detail: `${entry.duration_ms}ms`,
    exitCode: entry.exit_code,
    riskLevel: entry.risk_level,
    changeId: entry.change_id ?? null,
    raw: {
      id: entry.id,
      reason: entry.reason ?? null,
      change_id: entry.change_id ?? null,
      source: entry.source ?? null,
    },
  };
}

function formatTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleTimeString();
}

function needsAttention(item: ActivityItem): boolean {
  if (item.source === "desktop") return false;
  const risk = item.riskLevel?.toLowerCase();
  if (risk === "high" || risk === "blocked") return true;
  return item.kind.includes("approval");
}

export default function LiveActivityPanel({ audit }: Props) {
  const [events, setEvents] = useState<AgentEvent[]>([]);
  const [recentAudit, setRecentAudit] = useState<AuditEntry[]>(audit.slice(0, 20));
  const [status, setStatus] = useState<"connecting" | "live" | "offline">("connecting");
  const [error, setError] = useState<string | null>(null);
  const [sourceFilter, setSourceFilter] = useState("all");
  const [kindFilter, setKindFilter] = useState("all");
  const [search, setSearch] = useState("");
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());

  useEffect(() => {
    setRecentAudit(audit.slice(0, 20));
  }, [audit]);

  useEffect(() => {
    const controller = new AbortController();
    setStatus("connecting");
    setError(null);

    api
      .subscribeEvents((event) => {
        setStatus("live");
        setEvents((prev) => [event, ...prev].slice(0, MAX_EVENTS));
      }, controller.signal)
      .catch((err) => {
        if (!controller.signal.aborted) {
          setStatus("offline");
          setError(String(err));
        }
      });

    return () => controller.abort();
  }, []);

  useEffect(() => {
    let active = true;

    async function pollAudit() {
      try {
        const list = await api.listAudit({ limit: 20 });
        if (active) setRecentAudit(list);
      } catch {
        // Activity stream can still be useful without audit polling.
      }
    }

    const id = setInterval(pollAudit, AUDIT_POLL_MS);
    pollAudit();
    return () => {
      active = false;
      clearInterval(id);
    };
  }, []);

  const allItems = useMemo(() => {
    const byId = new Map<string, ActivityItem>();
    for (const item of events.map(eventToItem)) byId.set(item.id, item);
    for (const item of recentAudit.map(auditToItem)) byId.set(item.id, item);
    return [...byId.values()]
      .sort((a, b) => new Date(b.ts).getTime() - new Date(a.ts).getTime())
      .slice(0, 50);
  }, [events, recentAudit]);

  const sourceOptions = useMemo(
    () => [...new Set(allItems.map((item) => item.source).filter(Boolean))].sort(),
    [allItems],
  );

  const kindOptions = useMemo(
    () => [...new Set(allItems.map((item) => item.kind).filter(Boolean))].sort(),
    [allItems],
  );

  const items = useMemo(() => {
    const needle = search.trim().toLowerCase();
    return allItems.filter((item) => {
      if (sourceFilter !== "all" && item.source !== sourceFilter) return false;
      if (kindFilter !== "all" && item.kind !== kindFilter) return false;
      if (!needle) return true;
      return [
        item.source,
        item.kind,
        item.host,
        item.command,
        item.detail,
        item.riskLevel,
        item.changeId ?? undefined,
        item.sessionId,
      ]
        .filter(Boolean)
        .some((value) => value!.toLowerCase().includes(needle));
    });
  }, [allItems, kindFilter, search, sourceFilter]);

  const attentionItem = useMemo(
    () => allItems.find((item) => needsAttention(item)),
    [allItems],
  );

  function toggleExpanded(id: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  return (
    <section className="panel live-activity-panel">
      <div className="panel-title">
        <Activity size={16} />
        Live Agent Activity
        <span className={`activity-status ${status}`}>{status}</span>
      </div>

      <div className="activity-note">
        <ShieldAlert size={14} />
        Local daemon events stream live. Recent audit records catch CLI/MCP execs
        that wrote to the same config directory.
      </div>

      {error && <div className="error compact">{error}</div>}

      {attentionItem && (
        <div className="activity-alert">
          <ShieldAlert size={15} />
          <div>
            <strong>{attentionItem.source}</strong>
            <span>
              {attentionItem.kind}
              {attentionItem.host ? ` on ${attentionItem.host}` : ""}
              {attentionItem.riskLevel ? ` (${attentionItem.riskLevel})` : ""}
            </span>
          </div>
        </div>
      )}

      <div className="activity-filters">
        <label className="activity-search">
          <Search size={14} />
          <input
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder="Search activity"
          />
        </label>
        <select value={sourceFilter} onChange={(event) => setSourceFilter(event.target.value)}>
          <option value="all">All sources</option>
          {sourceOptions.map((source) => (
            <option key={source} value={source}>
              {source}
            </option>
          ))}
        </select>
        <select value={kindFilter} onChange={(event) => setKindFilter(event.target.value)}>
          <option value="all">All types</option>
          {kindOptions.map((kind) => (
            <option key={kind} value={kind}>
              {kind}
            </option>
          ))}
        </select>
      </div>

      <div className="activity-list">
        {items.length === 0 && (
          <div className="empty">
            <RefreshCw size={14} />
            Waiting for SSH activity
          </div>
        )}
        {items.map((item) => (
          <article className="activity-row" key={item.id}>
            <div className="activity-main">
              <div className="activity-meta">
                <button
                  className="activity-toggle"
                  type="button"
                  onClick={() => toggleExpanded(item.id)}
                  aria-label={expanded.has(item.id) ? "Collapse details" : "Expand details"}
                >
                  {expanded.has(item.id) ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                </button>
                <span>{formatTime(item.ts)}</span>
                <span>{item.source}</span>
                <span>{item.kind}</span>
                {item.host && <strong>{item.host}</strong>}
                {item.sessionId && <span>{item.sessionId.slice(0, 8)}</span>}
                {item.exitCode !== undefined && (
                  <span className={item.exitCode === 0 ? "ok" : "fail"}>
                    exit {item.exitCode ?? "?"}
                  </span>
                )}
                {item.riskLevel && <span>{item.riskLevel}</span>}
                {item.changeId && <span>{item.changeId}</span>}
              </div>
              {item.command && <code className="activity-command">{item.command}</code>}
              {item.detail && <pre>{item.detail}</pre>}
              {expanded.has(item.id) && (
                <div className="activity-details">
                  <dl>
                    <div>
                      <dt>time</dt>
                      <dd>{item.ts}</dd>
                    </div>
                    {item.host && (
                      <div>
                        <dt>host</dt>
                        <dd>{item.host}</dd>
                      </div>
                    )}
                    {item.sessionId && (
                      <div>
                        <dt>session</dt>
                        <dd>{item.sessionId}</dd>
                      </div>
                    )}
                    {item.changeId && (
                      <div>
                        <dt>change</dt>
                        <dd>{item.changeId}</dd>
                      </div>
                    )}
                  </dl>
                  {item.raw && <pre>{JSON.stringify(item.raw, null, 2)}</pre>}
                </div>
              )}
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}
