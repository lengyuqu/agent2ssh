import { Activity, BarChart3, Flame, Radar } from "lucide-react";
import { memo, Fragment, useMemo } from "react";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { useI18n } from "../i18n";
import type { AuditEntry, RiskLevel } from "../types";
import { Card } from "./ui/card";
import { EmptyState } from "./ui/state";
import { cn } from "../lib/utils";
import { formatClockTime } from "../lib/format";
import {
  type AuditRange,
  AUDIT_RANGE_MS,
  AUDIT_UNKNOWN_SOURCE,
  auditEntryInRange,
  auditEntryMatchesHost,
  auditEntryMatchesRisk,
  auditEntryMatchesSource,
  auditSourceKey,
} from "./auditFilters";

export type { AuditRange };

const RANGE_LABEL: Record<AuditRange, string> = { "24h": "24h", "7d": "7d", "30d": "30d" };
const HEATMAP_TOP_HOSTS = 8;

const RISK_ORDER: RiskLevel[] = ["low", "medium", "high", "blocked"];
// v3: chart colors stay on the risk families via the token layer (inline var()
// is allowed where class names can't reach — recharts needs raw color strings).
const RISK_COLOR: Record<RiskLevel, string> = {
  low: "var(--risk-low)",
  medium: "var(--risk-medium)",
  high: "var(--risk-high)",
  blocked: "var(--risk-high)",
};

function tooltipBoxCls() {
  return "rounded-md border border-edge bg-popover px-2.5 py-1.5 text-xs text-popover-foreground";
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

function RiskTooltip({ active, payload }: { active?: boolean; payload?: Array<{ payload: { label: string; count: number } }> }) {
  if (!active || !payload?.length) return null;
  const { label, count } = payload[0].payload;
  return (
    <div className={tooltipBoxCls()}>
      <div className="text-muted-foreground">{label}</div>
      <div className="font-semibold">{count}</div>
    </div>
  );
}

type Props = {
  entries: AuditEntry[];
  range: AuditRange;
  onRangeChange: (range: AuditRange) => void;
  activeRisk?: RiskLevel | "";
  activeSource?: string;
  activeHost?: string;
  onSelectRisk?: (level: RiskLevel) => void;
  onSelectSource?: (source: string) => void;
  onSelectHost?: (host: string) => void;
  loading?: boolean;
};

/** V2-3: audit visualizations. Entries and the shared range come from AuditWorkbench. */
function AuditCharts({
  entries,
  range,
  onRangeChange,
  activeRisk = "",
  activeSource = "",
  activeHost = "",
  onSelectRisk,
  onSelectSource,
  onSelectHost,
  loading = false,
}: Props) {
  const { t } = useI18n();

  const windowed = useMemo(
    () => entries.filter((entry) => auditEntryInRange(entry, range)),
    [entries, range],
  );

  const keep = (entry: AuditEntry, skip: "risk" | "source" | "host" | "none") => {
    if (skip !== "host" && !auditEntryMatchesHost(entry, activeHost)) return false;
    if (skip !== "source" && !auditEntryMatchesSource(entry, activeSource)) return false;
    if (skip !== "risk" && !auditEntryMatchesRisk(entry, activeRisk)) return false;
    return true;
  };

  const trend = useMemo(() => {
    const scoped = windowed.filter((entry) => keep(entry, "none"));
    const buckets = new Map<string, number>();
    const bucketMs = range === "24h" ? 60 * 60 * 1000 : 24 * 60 * 60 * 1000;
    const bucketCount = range === "24h" ? 24 : range === "7d" ? 7 : 30;
    const now = Date.now();
    for (let i = bucketCount - 1; i >= 0; i--) {
      const bucketStart = now - i * bucketMs;
      const date = new Date(bucketStart);
      const key =
        range === "24h"
          ? formatClockTime(date, { hour: "2-digit" })
          : date.toLocaleDateString([], { month: "short", day: "numeric" });
      buckets.set(key, 0);
    }
    const keys = [...buckets.keys()];
    for (const entry of scoped) {
      const ts = new Date(entry.ts).getTime();
      const stepsAgo = Math.floor((now - ts) / bucketMs);
      const idx = bucketCount - 1 - stepsAgo;
      if (idx >= 0 && idx < keys.length) {
        buckets.set(keys[idx], (buckets.get(keys[idx]) ?? 0) + 1);
      }
    }
    return keys.map((label) => ({ label, count: buckets.get(label) ?? 0 }));
    // keep() closes over the active filters; listing them keeps the buckets honest.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [windowed, range, activeHost, activeSource, activeRisk]);

  const riskDistribution = useMemo(() => {
    const scoped = windowed.filter((entry) => keep(entry, "risk"));
    const counts = new Map<RiskLevel, number>();
    for (const entry of scoped) counts.set(entry.risk_level, (counts.get(entry.risk_level) ?? 0) + 1);
    return RISK_ORDER.map((level) => ({
      level,
      label: t(level),
      count: counts.get(level) ?? 0,
      fill: RISK_COLOR[level],
    }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [windowed, activeHost, activeSource, t]);

  const sourceRanking = useMemo(() => {
    const scoped = windowed.filter((entry) => keep(entry, "source"));
    const counts = new Map<string, number>();
    for (const entry of scoped) {
      const source = auditSourceKey(entry);
      counts.set(source, (counts.get(source) ?? 0) + 1);
    }
    return [...counts.entries()]
      .map(([source, count]) => ({
        source,
        label: source === AUDIT_UNKNOWN_SOURCE ? t("unknown") : source,
        count,
      }))
      .sort((a, b) => b.count - a.count)
      .slice(0, 8);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [windowed, activeHost, activeRisk, t]);

  const heatmap = useMemo(() => {
    const scoped = windowed.filter((entry) => keep(entry, "host"));
    const totalsByHost = new Map<string, number>();
    for (const entry of scoped) totalsByHost.set(entry.host, (totalsByHost.get(entry.host) ?? 0) + 1);
    const topHosts = [...totalsByHost.entries()]
      .sort((a, b) => b[1] - a[1])
      .slice(0, HEATMAP_TOP_HOSTS)
      .map(([host]) => host);
    const topHostSet = new Set(topHosts);

    const grid = new Map<string, number>();
    let max = 0;
    for (const entry of scoped) {
      if (!topHostSet.has(entry.host)) continue;
      const hour = new Date(entry.ts).getHours();
      const key = `${entry.host}:${hour}`;
      const next = (grid.get(key) ?? 0) + 1;
      grid.set(key, next);
      if (next > max) max = next;
    }
    return { hosts: topHosts, grid, max };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [windowed, activeSource, activeRisk]);

  const hasData = windowed.length > 0;
  const sourceMax = sourceRanking[0]?.count ?? 0;

  return (
    <Card className="space-y-4 p-4">
      <div className="flex flex-wrap items-center gap-2 font-semibold">
        <Activity size={16} className="text-muted-foreground" />
        {t("Audit trends")}
        <div className="ml-auto flex gap-1 rounded-md border border-border bg-muted/40 p-0.5">
          {(Object.keys(AUDIT_RANGE_MS) as AuditRange[]).map((key) => (
            <button
              key={key}
              type="button"
              onClick={() => onRangeChange(key)}
              className={cn(
                "cursor-pointer rounded px-2.5 py-1 text-xs font-semibold transition-colors",
                range === key ? "bg-card text-foreground" : "text-muted-foreground hover:text-foreground"
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
                <AreaChart data={trend} margin={{ top: 4, right: 8, left: 0, bottom: 0 }}>
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
                <BarChart data={riskDistribution} margin={{ top: 4, right: 8, left: 0, bottom: 0 }}>
                  <CartesianGrid vertical={false} stroke="var(--border)" />
                  <XAxis
                    dataKey="label"
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
                  <Bar
                    dataKey="count"
                    radius={[4, 4, 0, 0]}
                    maxBarSize={40}
                    className="cursor-pointer"
                    cursor="pointer"
                    onClick={(item) => {
                      const level = item.payload?.level as RiskLevel | undefined;
                      if (level) onSelectRisk?.(level);
                    }}
                  >
                    {riskDistribution.map((row) => {
                      const selected = activeRisk === row.level;
                      return (
                        <Cell
                          key={row.level}
                          fill={row.fill}
                          cursor="pointer"
                          className="cursor-pointer"
                          stroke={selected ? "var(--foreground)" : "transparent"}
                          strokeWidth={selected ? 2 : 0}
                        />
                      );
                    })}
                  </Bar>
                </BarChart>
              </ResponsiveContainer>
            </div>
          </div>

          <div className="grid gap-2">
            <div className="flex items-center gap-1.5 text-xs font-bold uppercase tracking-wide text-muted-foreground">
              <Radar size={13} />
              {t("Executions by source")}
            </div>
            <div className="grid gap-1">
              {sourceRanking.map((row) => {
                const selected = activeSource === row.source;
                const width = sourceMax > 0 ? Math.round((row.count / sourceMax) * 100) : 0;
                return (
                  <button
                    key={row.source}
                    type="button"
                    aria-label={row.label}
                    aria-pressed={selected}
                    onClick={() => onSelectSource?.(row.source)}
                    className={cn(
                      "grid cursor-pointer appearance-none grid-cols-[90px_1fr_auto] items-center gap-2 rounded-md border px-2 py-1 text-left text-xs",
                      selected
                        ? "border-primary bg-primary/10 text-foreground"
                        : "border-transparent text-foreground hover:bg-muted",
                    )}
                  >
                    <span className="truncate font-medium">{row.label}</span>
                    <span aria-hidden className="h-2 overflow-hidden rounded-full bg-muted">
                      <span className="block h-full rounded-full bg-primary" style={{ width: `${width}%` }} />
                    </span>
                    <span aria-hidden className="font-mono text-muted-foreground">{row.count}</span>
                  </button>
                );
              })}
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
                  {heatmap.hosts.map((host) => {
                    const selected = activeHost === host;
                    return (
                      <Fragment key={host}>
                        <button
                          type="button"
                          title={host}
                          aria-pressed={selected}
                          onClick={() => onSelectHost?.(host)}
                          className={cn(
                            "min-w-0 cursor-pointer appearance-none truncate border-0 bg-transparent p-0 pr-1.5 text-left text-xs",
                            selected ? "font-semibold text-primary" : "text-foreground/85",
                          )}
                        >
                          {host}
                        </button>
                        {Array.from({ length: 24 }, (_, hour) => {
                          const count = heatmap.grid.get(`${host}:${hour}`) ?? 0;
                          const intensity = heatmap.max > 0 ? Math.round((count / heatmap.max) * 90) + 10 : 0;
                          return (
                            <button
                              key={`${host}-${hour}`}
                              type="button"
                              title={`${host} · ${hour}:00 · ${t("{count} executions", { count })}`}
                              aria-label={`${host} ${hour}:00`}
                              aria-pressed={selected}
                              onClick={() => onSelectHost?.(host)}
                              className={cn(
                                "aspect-square cursor-pointer appearance-none rounded-[2px] border-0 p-0",
                                selected && "ring-2 ring-primary",
                              )}
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
                    );
                  })}
                </div>
              </div>
            )}
          </div>
        </div>
      )}
    </Card>
  );
}

export default memo(AuditCharts);
