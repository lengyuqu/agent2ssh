import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, reportError } from "../api";
import { useAgentEvents } from "../eventsBus";
import type { AuditEntry, RiskLevel } from "../types";
import AuditCharts, { type AuditRange } from "./AuditCharts";
import AuditPanel from "./AuditPanel";
import {
  AUDIT_RANGE_MS,
  auditEntryInRange,
  auditEntryMatchesHost,
  auditEntryMatchesSearch,
  auditEntryMatchesSource,
} from "./auditFilters";

const FETCH_LIMIT = 5000;

type Filters = {
  range: AuditRange;
  host: string;
  riskLevel: RiskLevel | "";
  source: string;
  search: string;
};

const INITIAL_FILTERS: Filters = {
  range: "24h",
  host: "",
  riskLevel: "",
  source: "",
  search: "",
};

type Props = {
  onReplay?: (entry: AuditEntry) => void;
  onEditAndExecute?: (entry: AuditEntry) => void;
};

/** Shared audit filters and one list fetch for the charts and the table. */
function AuditWorkbench({ onReplay, onEditAndExecute }: Props) {
  const [filters, setFilters] = useState<Filters>(INITIAL_FILTERS);
  const [entries, setEntries] = useState<AuditEntry[]>([]);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    try {
      const since = new Date(Date.now() - AUDIT_RANGE_MS["30d"]).toISOString();
      const list = await api.listAudit({ since, limit: FETCH_LIMIT });
      setEntries(list);
    } catch (err) {
      reportError("audit-workbench", "load audit data failed", err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  // Coalesced: a multi-host exec fires one completion event per host.
  const reloadTimer = useRef<number | null>(null);
  useAgentEvents((event) => {
    if (event.event_type !== "exec_completed" || reloadTimer.current !== null) return;
    reloadTimer.current = window.setTimeout(() => {
      reloadTimer.current = null;
      void load();
    }, 1500);
  });
  useEffect(
    () => () => {
      if (reloadTimer.current !== null) window.clearTimeout(reloadTimer.current);
    },
    [],
  );

  const searched = useMemo(
    () => entries.filter((entry) => auditEntryMatchesSearch(entry, filters.search)),
    [entries, filters.search],
  );

  // Chip counts and the table start from this set: range, host, source and
  // search are applied; risk is not, so selecting high does not zero the others.
  const riskBasis = useMemo(
    () =>
      searched.filter(
        (entry) =>
          auditEntryInRange(entry, filters.range) &&
          auditEntryMatchesHost(entry, filters.host) &&
          auditEntryMatchesSource(entry, filters.source),
      ),
    [searched, filters.range, filters.host, filters.source],
  );

  const onRangeChange = useCallback((range: AuditRange) => {
    setFilters((current) => ({ ...current, range }));
  }, []);
  const onSelectRisk = useCallback((level: RiskLevel) => {
    setFilters((current) => ({
      ...current,
      riskLevel: current.riskLevel === level ? "" : level,
    }));
  }, []);
  const onSelectSource = useCallback((source: string) => {
    setFilters((current) => ({
      ...current,
      source: current.source === source ? "" : source,
    }));
  }, []);
  const onSelectHost = useCallback((host: string) => {
    setFilters((current) => ({
      ...current,
      host: current.host === host ? "" : host,
    }));
  }, []);
  const onHostChange = useCallback((host: string) => {
    setFilters((current) => ({ ...current, host }));
  }, []);
  const onRiskLevelChange = useCallback((riskLevel: RiskLevel | "") => {
    setFilters((current) => ({ ...current, riskLevel }));
  }, []);
  const onSourceChange = useCallback((source: string) => {
    setFilters((current) => ({ ...current, source }));
  }, []);
  const onSearchChange = useCallback((search: string) => {
    setFilters((current) => ({ ...current, search }));
  }, []);
  const onClearFilters = useCallback(() => {
    setFilters((current) => ({
      ...current,
      host: "",
      riskLevel: "",
      source: "",
      search: "",
    }));
  }, []);

  return (
    <div className="grid gap-[18px]">
      <AuditCharts
        entries={searched}
        loading={loading}
        range={filters.range}
        onRangeChange={onRangeChange}
        activeRisk={filters.riskLevel}
        activeSource={filters.source}
        activeHost={filters.host}
        onSelectRisk={onSelectRisk}
        onSelectSource={onSelectSource}
        onSelectHost={onSelectHost}
      />
      <AuditPanel
        audit={riskBasis}
        loading={loading}
        onRefresh={load}
        onReplay={onReplay}
        onEditAndExecute={onEditAndExecute}
        host={filters.host}
        riskLevel={filters.riskLevel}
        source={filters.source}
        search={filters.search}
        onHostChange={onHostChange}
        onRiskLevelChange={onRiskLevelChange}
        onSourceChange={onSourceChange}
        onSearchChange={onSearchChange}
        onClearFilters={onClearFilters}
      />
    </div>
  );
}

export default memo(AuditWorkbench);
