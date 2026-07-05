import { Activity, BarChart3, Flame, Radar } from "lucide-react";
import { Fragment, useEffect, useMemo, useState } from "react";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { api, reportError } from "../api";
import { useAgentEvents } from "../eventsBus";
import { useI18n } from "../i18n";
import type { AuditEntry, RiskLevel } from "../types";
import { Card } from "./ui/card";
import { EmptyState } from "./ui/state";
import { cn } from "../lib/utils";

type RangeKey = "24h" | "7d" | "30d";

const RANGE_MS: Record<RangeKey, number> = {
  "24h": 24 * 60 * 60 * 1000,
  "7d": 7 * 24 * 60 * 60 * 1000,
  "30d": 30 * 24 * 60 * 60 * 1000,
};

const RANGE_LABEL: Record<RangeKey, string> = { "24h": "24h", "7d": "7d", "30d": "30d" };

// Dashboard already fetches a 30-day window with the same cap for its 24h
// counter; reuse that convention here rather than depending on whatever limit
// the AuditPanel filter below happens to be set to.
const FETCH_LIMIT = 5000;
const HEATMAP_TOP_HOSTS = 8;

const RISK_ORDER: RiskLevel[] = ["low", "medium", "high", "blocked"];
const RISK_COLOR: Record<RiskLevel, string> = {
  low: "var(--success)",
  medium: "var(--warning)",
  high: "var(--destructive)",
  blocked: "var(--destructive)",
};

function tooltipBoxCls() {
  return "rounded-md border border-border bg-popover px-2.5 py-1.5 text-xs text-popover-foreground shadow-lg";
}

function TrendTooltip({ active, payload, label }: { active?: boolean; payload?: Array<{ value: number }>; label?: string }) {
  if (!active || !payload?.length) return null;
  return (
    <div className={tooltipBoxCls()}>
      <div className="text-muted-foreground">{label}</div>
      <div className="font-semibold">{payload[0].value}</div>
    </div>
  );
}

function RiskTooltip({ active, payload }: { active?: boolean; payload?: Array<{ payload: { level: string; count: number } }> }) {
  if (!active || !payload?.length) return null;
  const { level, count } = payload[0].payload;
  return (
    <div className={tooltipBoxCls()}>
      <div className="text-muted-foreground">{level}</div>
      <div className="font-semibold">{count}</div>
    </div>
  );
}

function SourceTooltip({ active, payload }: { active?: boolean; payload?: Array<{ payload: { source: string; count: number } }> }) {
  if (!active || !payload?.length) return null;
  const { source, count } = payload[0].payload;
  return (
    <div className={tooltipBoxCls()}>
      <div className="text-muted-foreground">{source}</div>
      <div className="font-semibold">{count}</div>
    </div>
  );
}

/** V2-3: audit visualizations — execution trend, risk distribution, source ranking, host×hour heatmap. */
export default function AuditCharts() {
  const { t } = useI18n();
  const [entries, setEntries] = useState<AuditEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [range, setRange] = useState<RangeKey>("24h");

  const load = async () => {
    try {
      const since = new Date(Date.now() - RANGE_MS["30d"]).toISOString();
      const list = await api.listAudit({ since, limit: FETCH_LIMIT });
      setEntries(list);
    } catch (err) {
      reportError("audit-charts", "load audit data for charts failed", err);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

  // Nudge a refresh when a new exec lands so the trend line keeps pace with
  // live activity instead of only updating when the module is re-entered.
  useAgentEvents((event) => {
    if (event.event_type === "exec_completed") load();
  });

  const windowed = useMemo(() => {
    const cutoff = Date.now() - RANGE_MS[range];
    return entries.filter((entry) => new Date(entry.ts).getTime() >= cutoff);
  }, [entries, range]);

  const trend = useMemo(() => {
    const buckets = new Map<string, number>();
    const bucketMs = range === "24h" ? 60 * 60 * 1000 : 24 * 60 * 60 * 1000;
    const bucketCount = range === "24h" ? 24 : range === "7d" ? 7 : 30;
    const now = Date.now();
    for (let i = bucketCount - 1; i >= 0; i--) {
      const bucketStart = now - i * bucketMs;
      const date = new Date(bucketStart);
      const key =
        range === "24h"
          ? date.toLocaleTimeString([], { hour: "2-digit" })
          : date.toLocaleDateString([], { month: "short", day: "numeric" });
      buckets.set(key, 0);
    }
    const keys = [...buckets.keys()];
    for (const entry of windowed) {
      const ts = new Date(entry.ts).getTime();
      const stepsAgo = Math.floor((now - ts) / bucketMs);
      const idx = bucketCount - 1 - stepsAgo;
      if (idx >= 0 && idx < keys.length) {
        buckets.set(keys[idx], (buckets.get(keys[idx]) ?? 0) + 1);
      }
    }
    return keys.map((label) => ({ label, count: buckets.get(label) ?? 0 }));
  }, [windowed, range]);

  const riskDistribution = useMemo(
    () =>
      RISK_ORDER.map((level) => ({
        level: t(level),
        count: windowed.filter((entry) => entry.risk_level === level).length,
        fill: RISK_COLOR[level],
      })),
    [windowed, t]
  );

  const sourceRanking = useMemo(() => {
    const counts = new Map<string, number>();
    for (const entry of windowed) {
      const source = entry.source ?? t("unknown");
      counts.set(source, (counts.get(source) ?? 0) + 1);
    }
    return [...counts.entries()]
      .map(([source, count]) => ({ source, count }))
      .sort((a, b) => b.count - a.count)
      .slice(0, 8);
  }, [windowed, t]);

  const heatmap = useMemo(() => {
    const totalsByHost = new Map<string, number>();
    for (const entry of windowed) totalsByHost.set(entry.host, (totalsByHost.get(entry.host) ?? 0) + 1);
    const topHosts = [...totalsByHost.entries()]
      .sort((a, b) => b[1] - a[1])
      .slice(0, HEATMAP_TOP_HOSTS)
      .map(([host]) => host);

    const grid = new Map<string, number>();
    let max = 0;
    for (const entry of windowed) {
      if (!topHosts.includes(entry.host)) continue;
      const hour = new Date(entry.ts).getHours();
      const key = `${entry.host}:${hour}`;
      const next = (grid.get(key) ?? 0) + 1;
      grid.set(key, next);
      if (next > max) max = next;
    }
    return { hosts: topHosts, grid, max };
  }, [windowed]);

  const hasData = windowed.length > 0;

  return (
    <Card className="space-y-4 p-4">
      <div className="flex flex-wrap items-center gap-2 font-semibold">
        <Activity size={16} className="text-muted-foreground" />
        {t("Audit trends")}
        <div className="ml-auto flex gap-1 rounded-md border border-border bg-muted/40 p-0.5">
          {(Object.keys(RANGE_MS) as RangeKey[]).map((key) => (
            <button
              key={key}
              type="button"
              onClick={() => setRange(key)}
              className={cn(
                "rounded px-2.5 py-1 text-xs font-semibold transition-colors",
                range === key ? "bg-card text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground"
              )}
            >
              {RANGE_LABEL[key]}
            </button>
          ))}
        </div>
      </div>

      {!loading && !hasData && <EmptyState icon={Activity} title={t("No executions in this range")} />}

      {hasData && (
        <div className="grid gap-4 xl:grid-cols-2">
          <div className="grid gap-2">
            <div className="flex items-center gap-1.5 text-xs font-bold uppercase tracking-wide text-muted-foreground">
              <Activity size={13} />
              {t("Execution volume")}
            </div>
            <div className="h-[220px]">
              <ResponsiveContainer width="100%" height="100%">
                <AreaChart data={trend} margin={{ top: 4, right: 8, left: -20, bottom: 0 }}>
                  <CartesianGrid vertical={false} stroke="var(--border)" strokeDasharray="0" />
                  <XAxis
                    dataKey="label"
                    tick={{ fill: "var(--muted-foreground)", fontSize: 11 }}
                    axisLine={{ stroke: "var(--border)" }}
                    tickLine={false}
                    interval="preserveStartEnd"
                  />
                  <YAxis
                    allowDecimals={false}
                    tick={{ fill: "var(--muted-foreground)", fontSize: 11 }}
                    axisLine={false}
                    tickLine={false}
                    width={32}
                  />
                  <Tooltip content={<TrendTooltip />} cursor={{ stroke: "var(--border)" }} />
                  <Area
                    type="monotone"
                    dataKey="count"
                    stroke="var(--primary)"
                    strokeWidth={2}
                    fill="var(--primary)"
                    fillOpacity={0.1}
                  />
                </AreaChart>
              </ResponsiveContainer>
            </div>
          </div>

          <div className="grid gap-2">
            <div className="flex items-center gap-1.5 text-xs font-bold uppercase tracking-wide text-muted-foreground">
              <BarChart3 size={13} />
              {t("Risk distribution")}
            </div>
            <div className="h-[220px]">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={riskDistribution} margin={{ top: 4, right: 8, left: -20, bottom: 0 }}>
                  <CartesianGrid vertical={false} stroke="var(--border)" />
                  <XAxis
                    dataKey="level"
                    tick={{ fill: "var(--muted-foreground)", fontSize: 11 }}
                    axisLine={{ stroke: "var(--border)" }}
                    tickLine={false}
                  />
                  <YAxis
                    allowDecimals={false}
                    tick={{ fill: "var(--muted-foreground)", fontSize: 11 }}
                    axisLine={false}
                    tickLine={false}
                    width={32}
                  />
                  <Tooltip content={<RiskTooltip />} cursor={{ fill: "var(--muted)" }} />
                  <Bar dataKey="count" radius={[4, 4, 0, 0]} maxBarSize={40} />
                </BarChart>
              </ResponsiveContainer>
            </div>
          </div>

          <div className="grid gap-2">
            <div className="flex items-center gap-1.5 text-xs font-bold uppercase tracking-wide text-muted-foreground">
              <Radar size={13} />
              {t("Executions by source")}
            </div>
            <div style={{ height: Math.max(120, sourceRanking.length * 34) }}>
              <ResponsiveContainer width="100%" height="100%">
                <BarChart
                  data={sourceRanking}
                  layout="vertical"
                  margin={{ top: 4, right: 16, left: 8, bottom: 0 }}
                >
                  <CartesianGrid horizontal={false} stroke="var(--border)" />
                  <XAxis
                    type="number"
                    allowDecimals={false}
                    tick={{ fill: "var(--muted-foreground)", fontSize: 11 }}
                    axisLine={{ stroke: "var(--border)" }}
                    tickLine={false}
                  />
                  <YAxis
                    type="category"
                    dataKey="source"
                    tick={{ fill: "var(--foreground)", fontSize: 12 }}
                    axisLine={false}
                    tickLine={false}
                    width={90}
                  />
                  <Tooltip content={<SourceTooltip />} cursor={{ fill: "var(--muted)" }} />
                  <Bar dataKey="count" fill="var(--primary)" radius={[0, 4, 4, 0]} maxBarSize={18} />
                </BarChart>
              </ResponsiveContainer>
            </div>
          </div>

          <div className="grid gap-2">
            <div className="flex items-center gap-1.5 text-xs font-bold uppercase tracking-wide text-muted-foreground">
              <Flame size={13} />
              {t("Host activity by hour")}
            </div>
            {heatmap.hosts.length === 0 ? (
              <EmptyState icon={Flame} title={t("No host activity in this range")} />
            ) : (
              <div className="overflow-x-auto">
                <div
                  className="grid gap-[2px]"
                  style={{ gridTemplateColumns: `84px repeat(24, minmax(14px, 1fr))` }}
                >
                  <div />
                  {Array.from({ length: 24 }, (_, hour) => (
                    <div key={hour} className="text-center text-[9px] text-muted-foreground">
                      {hour % 3 === 0 ? hour : ""}
                    </div>
                  ))}
                  {heatmap.hosts.map((host) => (
                    <Fragment key={host}>
                      <div className="truncate pr-1.5 text-xs text-foreground/85" title={host}>
                        {host}
                      </div>
                      {Array.from({ length: 24 }, (_, hour) => {
                        const count = heatmap.grid.get(`${host}:${hour}`) ?? 0;
                        const intensity = heatmap.max > 0 ? Math.round((count / heatmap.max) * 90) + 10 : 0;
                        return (
                          <div
                            key={`${host}-${hour}`}
                            title={`${host} · ${hour}:00 · ${t("{count} executions", { count })}`}
                            className="aspect-square rounded-[2px]"
                            style={{
                              background:
                                count === 0
                                  ? "var(--muted)"
                                  : `color-mix(in srgb, var(--primary) ${intensity}%, var(--card))`,
                            }}
                          />
                        );
                      })}
                    </Fragment>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>
      )}
    </Card>
  );
}
