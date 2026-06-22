import { History, RefreshCw, SlidersHorizontal } from "lucide-react";
import { useState } from "react";
import { useI18n } from "../i18n";
import type { AuditEntry, AuditFilter, RiskLevel } from "../types";
import RiskBadge from "./RiskBadge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { IconButton } from "./ui/icon-button";
import { Input } from "./ui/input";
import { Select } from "./ui/select";
import { cn } from "../lib/utils";

const labelCls = "grid gap-1.5 text-sm font-medium text-foreground/90";

// J3: cap mounted rows so a large `limit` query never renders thousands of nodes.
const RENDER_CAP_STEP = 200;

type Props = {
  audit: AuditEntry[];
  onRefresh: (filter?: AuditFilter) => void | Promise<void>;
};

export default function AuditPanel({ audit, onRefresh }: Props) {
  const { t } = useI18n();
  const [showFilters, setShowFilters] = useState(false);
  const [hostFilter, setHostFilter] = useState("");
  const [riskFilter, setRiskFilter] = useState<RiskLevel | "">("");
  const [limit, setLimit] = useState(50);
  const [renderCap, setRenderCap] = useState(RENDER_CAP_STEP);

  function applyFilters() {
    setRenderCap(RENDER_CAP_STEP);
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
    setRenderCap(RENDER_CAP_STEP);
    onRefresh();
  }

  return (
    <Card className="space-y-3 p-4">
      <div className="flex items-center gap-2 font-semibold">
        <History size={16} className="text-muted-foreground" />
        {t("Audit")}
        <IconButton
          className="ml-auto"
          onClick={() => setShowFilters(!showFilters)}
          title={t("Toggle filters")}
        >
          <SlidersHorizontal size={15} />
        </IconButton>
        <IconButton onClick={() => onRefresh()} title={t("Refresh audit")}>
          <RefreshCw size={15} />
        </IconButton>
      </div>

      {showFilters && (
        <div className="grid grid-cols-[1fr_1fr_100px_auto] items-end gap-2.5 max-md:grid-cols-1">
          <label className={labelCls}>
            {t("Host")}
            <Input
              value={hostFilter}
              onChange={(e) => setHostFilter(e.target.value)}
              placeholder={t("all hosts")}
            />
          </label>
          <label className={labelCls}>
            {t("Risk level")}
            <Select
              value={riskFilter}
              onChange={(e) => setRiskFilter(e.target.value as RiskLevel | "")}
            >
              <option value="">{t("all")}</option>
              <option value="low">{t("low")}</option>
              <option value="medium">{t("medium")}</option>
              <option value="high">{t("high")}</option>
              <option value="blocked">{t("blocked")}</option>
            </Select>
          </label>
          <label className={labelCls}>
            {t("Limit")}
            <Input
              type="number"
              min={1}
              max={500}
              value={limit}
              onChange={(e) => setLimit(Number(e.target.value))}
            />
          </label>
          <div className="flex items-end gap-1.5">
            <Button onClick={applyFilters}>{t("Apply")}</Button>
            <Button variant="secondary" onClick={clearFilters}>
              {t("Clear")}
            </Button>
          </div>
        </div>
      )}

      <div className="grid gap-2">
        {audit.slice(0, renderCap).map((entry) => (
          <div
            key={entry.id}
            className={cn(
              "grid grid-cols-[180px_110px_minmax(0,1fr)_120px_auto] items-center gap-3 border-t border-border pt-2 max-md:grid-cols-1 max-md:gap-1",
              entry.risk_level === "high" &&
                "rounded-md border border-warning/40 bg-warning/10 px-2 py-1.5"
            )}
          >
            <span className="text-xs text-muted-foreground">
              {new Date(entry.ts).toLocaleString()}
            </span>
            <strong className="truncate font-semibold">{entry.host}</strong>
            <code className="truncate font-mono text-sm">{entry.command}</code>
            <em className="text-xs not-italic text-muted-foreground">
              exit={entry.exit_code ?? "signal"} {entry.duration_ms}ms
            </em>
            <RiskBadge level={entry.risk_level ?? "low"} hideLow />
          </div>
        ))}
        {audit.length > renderCap && (
          <button
            type="button"
            onClick={() => setRenderCap((c) => c + RENDER_CAP_STEP)}
            className="border-t border-border pt-2 text-center text-xs text-muted-foreground hover:text-foreground"
          >
            {t("Show more ({count} hidden)", { count: audit.length - renderCap })}
          </button>
        )}
        {audit.length === 0 && (
          <div className="px-1 py-2 text-sm text-muted-foreground">
            {t("No commands executed yet")}
          </div>
        )}
      </div>
    </Card>
  );
}
