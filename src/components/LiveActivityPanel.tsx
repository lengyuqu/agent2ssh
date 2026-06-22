import { Activity, ChevronDown, ChevronRight, RefreshCw, Search, ShieldAlert } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import type { AgentEvent, AuditEntry } from "../types";
import { Card } from "./ui/card";
import { Select } from "./ui/select";
import { cn } from "../lib/utils";

const MAX_EVENTS = 80;
const AUDIT_POLL_MS = 10000;
const EVENT_RECONNECT_MS = 3000;
const EVENT_CONNECT_TIMEOUT_MS = 6000;
const EVENT_BATCH_MS = 100;

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
  anomalyKind?: string;
  anomalyReason?: string;
  severity?: string;
  raw?: Record<string, unknown>;
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
  const anomalyReason = asString(data.reason);
  const anomalyKind = asString(data.kind);
  const severity = asString(data.severity);

  let detail = anomalyReason ?? output ?? input;
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
    anomalyKind,
    anomalyReason,
    severity,
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
  if (item.anomalyReason) return true;
  return item.kind.includes("approval");
}

const statusCls: Record<string, string> = {
  live: "bg-success/15 text-success",
  connecting: "bg-sky-500/15 text-sky-500",
  offline: "bg-destructive/15 text-destructive",
};

export default function LiveActivityPanel() {
  const { t } = useI18n();
  const [events, setEvents] = useState<AgentEvent[]>([]);
  const [recentAudit, setRecentAudit] = useState<AuditEntry[]>([]);
  const [status, setStatus] = useState<"connecting" | "live" | "offline">("connecting");
  const [error, setError] = useState<string | null>(null);
  const [sourceFilter, setSourceFilter] = useState("all");
  const [kindFilter, setKindFilter] = useState("all");
  const [search, setSearch] = useState("");
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const eventQueueRef = useRef<AgentEvent[]>([]);
  const flushTimerRef = useRef<number | null>(null);

  useEffect(() => {
    let active = true;
    let controller: AbortController | null = null;
    let retryTimer: number | null = null;
    let connectTimer: number | null = null;

    function flushEvents() {
      flushTimerRef.current = null;
      const queued = eventQueueRef.current;
      if (queued.length === 0) return;
      eventQueueRef.current = [];
      const newestFirst = queued.slice().reverse();
      setEvents((prev) => [...newestFirst, ...prev].slice(0, MAX_EVENTS));
    }

    function queueEvent(event: AgentEvent) {
      eventQueueRef.current.push(event);
      if (flushTimerRef.current !== null) return;
      flushTimerRef.current = window.setTimeout(flushEvents, EVENT_BATCH_MS);
    }

    function clearConnectTimer() {
      if (connectTimer !== null) {
        window.clearTimeout(connectTimer);
        connectTimer = null;
      }
    }

    function scheduleReconnect() {
      if (!active || retryTimer !== null) return;
      retryTimer = window.setTimeout(() => {
        retryTimer = null;
        connect();
      }, EVENT_RECONNECT_MS);
    }

    function connect() {
      controller?.abort();
      controller = new AbortController();
      setStatus("connecting");
      setError(null);

      connectTimer = window.setTimeout(() => {
        controller?.abort();
      }, EVENT_CONNECT_TIMEOUT_MS);

      api
        .subscribeEvents(
          (event) => {
            clearConnectTimer();
            setStatus("live");
            queueEvent(event);
          },
          controller.signal,
          () => {
            clearConnectTimer();
            setStatus("live");
          }
        )
        .catch((err) => {
          clearConnectTimer();
          if (!active) return;
          if (controller?.signal.aborted) {
            api.writeDiagnosticLog("warn", "activity", "event stream connect timed out or was aborted").catch(() => {});
            scheduleReconnect();
            return;
          }
          setStatus("offline");
          setError(String(err));
          api
            .writeDiagnosticLog("error", "activity", "event stream subscription failed", {
              error: String(err),
            })
            .catch(() => {});
          scheduleReconnect();
        });
    }

    connect();
    return () => {
      active = false;
      controller?.abort();
      clearConnectTimer();
      if (retryTimer !== null) window.clearTimeout(retryTimer);
      if (flushTimerRef.current !== null) window.clearTimeout(flushTimerRef.current);
      eventQueueRef.current = [];
    };
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
    [allItems]
  );

  const kindOptions = useMemo(
    () => [...new Set(allItems.map((item) => item.kind).filter(Boolean))].sort(),
    [allItems]
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
        item.anomalyKind,
        item.anomalyReason,
        item.severity,
        item.changeId ?? undefined,
        item.sessionId,
      ]
        .filter(Boolean)
        .some((value) => value!.toLowerCase().includes(needle));
    });
  }, [allItems, kindFilter, search, sourceFilter]);

  const attentionItem = useMemo(
    () => allItems.find((item) => needsAttention(item)),
    [allItems]
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
    <Card className="space-y-3 p-4">
      <div className="flex items-center gap-2 font-semibold">
        <Activity size={16} className="text-muted-foreground" />
        {t("Live Agent Activity")}
        <span
          className={cn(
            "ml-auto rounded-full px-2 py-0.5 text-[11px] font-bold uppercase tracking-wide",
            statusCls[status]
          )}
        >
          {t(status)}
        </span>
      </div>

      <div className="flex items-start gap-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-sm text-warning">
        <ShieldAlert size={14} className="mt-0.5 shrink-0" />
        {t("Local daemon events stream live. Recent audit records catch CLI/MCP execs that wrote to the same config directory.")}
      </div>

      {error && (
        <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {error}
        </div>
      )}

      {attentionItem && (
        <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2.5 text-destructive">
          <ShieldAlert size={15} className="mt-0.5 shrink-0" />
          <div className="grid gap-0.5">
            <strong className="break-words">{attentionItem.source}</strong>
            <span className="break-words text-sm">
              {attentionItem.kind}
              {attentionItem.host ? ` on ${attentionItem.host}` : ""}
              {attentionItem.riskLevel ? ` (${attentionItem.riskLevel})` : ""}
              {attentionItem.anomalyReason ? `: ${attentionItem.anomalyReason}` : ""}
            </span>
          </div>
        </div>
      )}

      <div className="grid grid-cols-[minmax(0,1fr)_minmax(120px,0.35fr)_minmax(120px,0.35fr)] gap-2 max-sm:grid-cols-1">
        <label className="flex h-9 items-center gap-1.5 rounded-md border border-input bg-card px-3">
          <Search size={14} className="shrink-0 text-muted-foreground" />
          <input
            className="w-full min-w-0 border-0 bg-transparent text-sm text-foreground outline-none placeholder:text-muted-foreground"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={t("Search activity")}
          />
        </label>
        <Select value={sourceFilter} onChange={(event) => setSourceFilter(event.target.value)}>
          <option value="all">{t("All sources")}</option>
          {sourceOptions.map((source) => (
            <option key={source} value={source}>
              {source}
            </option>
          ))}
        </Select>
        <Select value={kindFilter} onChange={(event) => setKindFilter(event.target.value)}>
          <option value="all">{t("All types")}</option>
          {kindOptions.map((kind) => (
            <option key={kind} value={kind}>
              {kind}
            </option>
          ))}
        </Select>
      </div>

      <div className="grid max-h-[360px] gap-2 overflow-auto">
        {items.length === 0 && (
          <div className="flex items-center gap-2 px-1 py-2 text-sm text-muted-foreground">
            <RefreshCw size={14} />
            {t("Waiting for SSH activity")}
          </div>
        )}
        {items.map((item) => (
          <article
            className={cn(
              "rounded-lg border bg-card p-3",
              item.anomalyReason ? "border-warning/50 bg-warning/5" : "border-border"
            )}
            key={item.id}
          >
            <div className="grid gap-1.5">
              <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                <button
                  className="inline-flex size-[22px] items-center justify-center rounded-md border border-border bg-card text-muted-foreground transition-colors hover:bg-muted"
                  type="button"
                  onClick={() => toggleExpanded(item.id)}
                  aria-label={expanded.has(item.id) ? t("Collapse details") : t("Expand details")}
                >
                  {expanded.has(item.id) ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                </button>
                <span>{formatTime(item.ts)}</span>
                <span>{item.source}</span>
                <span>{item.kind}</span>
                {item.host && <strong className="font-semibold text-foreground">{item.host}</strong>}
                {item.sessionId && <span>{item.sessionId.slice(0, 8)}</span>}
                {item.exitCode !== undefined && (
                  <span
                    className={cn(
                      "font-semibold",
                      item.exitCode === 0 ? "text-success" : "text-destructive"
                    )}
                  >
                    exit {item.exitCode ?? "?"}
                  </span>
                )}
                {item.riskLevel && <span>{item.riskLevel}</span>}
                {item.severity && (
                  <span className="font-bold uppercase text-orange-600">{item.severity}</span>
                )}
                {item.anomalyKind && <span>{item.anomalyKind.split("_").join(" ")}</span>}
                {item.changeId && <span>{item.changeId}</span>}
              </div>
              {item.command && (
                <code className="block break-all rounded bg-muted px-2 py-1.5 text-xs text-foreground">
                  {item.command}
                </code>
              )}
              {item.detail && (
                <pre className="m-0 max-h-[150px] overflow-auto whitespace-pre-wrap break-words rounded bg-[#0f172a] p-2 text-xs text-slate-200">
                  {item.detail}
                </pre>
              )}
              {expanded.has(item.id) && (
                <div className="grid gap-2 border-t border-border pt-2">
                  <dl className="grid gap-1.5">
                    <div className="grid grid-cols-[72px_minmax(0,1fr)] gap-2">
                      <dt className="text-[11px] uppercase text-muted-foreground">{t("time")}</dt>
                      <dd className="m-0 break-words">{item.ts}</dd>
                    </div>
                    {item.host && (
                      <div className="grid grid-cols-[72px_minmax(0,1fr)] gap-2">
                        <dt className="text-[11px] uppercase text-muted-foreground">{t("host")}</dt>
                        <dd className="m-0 break-words">{item.host}</dd>
                      </div>
                    )}
                    {item.sessionId && (
                      <div className="grid grid-cols-[72px_minmax(0,1fr)] gap-2">
                        <dt className="text-[11px] uppercase text-muted-foreground">
                          {t("session")}
                        </dt>
                        <dd className="m-0 break-words">{item.sessionId}</dd>
                      </div>
                    )}
                    {item.changeId && (
                      <div className="grid grid-cols-[72px_minmax(0,1fr)] gap-2">
                        <dt className="text-[11px] uppercase text-muted-foreground">
                          {t("change")}
                        </dt>
                        <dd className="m-0 break-words">{item.changeId}</dd>
                      </div>
                    )}
                  </dl>
                  {item.raw && (
                    <pre className="m-0 max-h-[150px] overflow-auto whitespace-pre-wrap break-words rounded bg-[#0f172a] p-2 text-xs text-slate-200">
                      {JSON.stringify(item.raw, null, 2)}
                    </pre>
                  )}
                </div>
              )}
            </div>
          </article>
        ))}
      </div>
    </Card>
  );
}
