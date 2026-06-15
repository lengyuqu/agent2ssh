import { Activity, RefreshCw, ShieldAlert } from "lucide-react";
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
  };
}

function auditToItem(entry: AuditEntry): ActivityItem {
  return {
    id: `audit-${entry.id}`,
    ts: entry.ts,
    source: "audit",
    kind: "exec recorded",
    host: entry.host,
    command: entry.command,
    detail: `${entry.duration_ms}ms`,
    exitCode: entry.exit_code,
    riskLevel: entry.risk_level,
    changeId: entry.change_id ?? null,
  };
}

function formatTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleTimeString();
}

export default function LiveActivityPanel({ audit }: Props) {
  const [events, setEvents] = useState<AgentEvent[]>([]);
  const [recentAudit, setRecentAudit] = useState<AuditEntry[]>(audit.slice(0, 20));
  const [status, setStatus] = useState<"connecting" | "live" | "offline">("connecting");
  const [error, setError] = useState<string | null>(null);

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

  const items = useMemo(() => {
    const byId = new Map<string, ActivityItem>();
    for (const item of events.map(eventToItem)) byId.set(item.id, item);
    for (const item of recentAudit.map(auditToItem)) byId.set(item.id, item);
    return [...byId.values()]
      .sort((a, b) => new Date(b.ts).getTime() - new Date(a.ts).getTime())
      .slice(0, 50);
  }, [events, recentAudit]);

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
                <span>{formatTime(item.ts)}</span>
                <span>{item.source}</span>
                <span>{item.kind}</span>
                {item.host && <strong>{item.host}</strong>}
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
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}
