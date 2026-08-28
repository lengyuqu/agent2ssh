import { Clipboard, History, RefreshCw, RotateCcw, SlidersHorizontal } from "lucide-react";
import { useMemo, useState } from "react";
import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  getSortedRowModel,
  useReactTable,
  type RowSelectionState,
  type SortingState,
} from "@tanstack/react-table";
import { useI18n } from "../i18n";
import type { AuditEntry, AuditFilter, RiskLevel } from "../types";
import RiskBadge from "./RiskBadge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { ColumnVisibilityMenu, SortIcon } from "./ui/data-table";
import { IconButton } from "./ui/icon-button";
import { Input } from "./ui/input";
import { Select } from "./ui/select";
import { EmptyState } from "./ui/state";
import { useToast } from "./ui/toast";
import { cn } from "../lib/utils";

const labelCls = "grid gap-1.5 text-sm font-medium text-foreground/90";

// v3: result three-state — approved/executed ok = low green, failed = high red,
// no exit recorded (auto/blocked paths) = text-3 faint.
const RESULT_DOT: Record<"ok" | "fail" | "none", string> = {
  ok: "bg-risk-low",
  fail: "bg-risk-high",
  none: "bg-muted-foreground/50",
};

// Command cells are tinted with the risk family of the entry (full-row, since
// matched-rule fragment data is not available from the backend yet).
const COMMAND_TINT: Record<RiskLevel, string> = {
  low: "text-risk-low",
  medium: "text-risk-medium",
  high: "text-risk-high",
  blocked: "text-risk-high",
};

// Filter chips: selected chip = solid risk color with canvas-0 text, idle = risk outline.
const CHIP_ACTIVE: Record<RiskLevel, string> = {
  low: "border-transparent bg-risk-low text-canvas-0",
  medium: "border-transparent bg-risk-medium text-canvas-0",
  high: "border-transparent bg-risk-high text-canvas-0",
  blocked: "border-transparent bg-risk-high text-canvas-0",
};

const CHIP_IDLE: Record<RiskLevel, string> = {
  low: "border-risk-low/50 text-risk-low hover:border-risk-low",
  medium: "border-risk-medium/50 text-risk-medium hover:border-risk-medium",
  high: "border-risk-high/50 text-risk-high hover:border-risk-high",
  blocked: "border-risk-high/50 text-risk-high hover:border-risk-high",
};

// J3: cap mounted rows so a large `limit` query never renders thousands of nodes.
const RENDER_CAP_STEP = 200;

type Props = {
  audit: AuditEntry[];
  onRefresh: (filter?: AuditFilter) => void | Promise<void>;
  /** v3 §3.3/§3.4: replay an audit row in the terminal drawer. Optional — the
   *  replay column only renders when a handler is provided. */
  onReplay?: (entry: AuditEntry) => void;
};

const columnHelper = createColumnHelper<AuditEntry>();

export default function AuditPanel({ audit, onRefresh, onReplay }: Props) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [showFilters, setShowFilters] = useState(false);
  const [hostFilter, setHostFilter] = useState("");
  const [riskFilter, setRiskFilter] = useState<RiskLevel | "">("");
  const [limit, setLimit] = useState(50);
  const [renderCap, setRenderCap] = useState(RENDER_CAP_STEP);
  const [sorting, setSorting] = useState<SortingState>([{ id: "ts", desc: true }]);
  const [rowSelection, setRowSelection] = useState<RowSelectionState>({});

  function parseLimit(value: string): number {
    const parsed = Number(value);
    if (!Number.isFinite(parsed)) return 50;
    return Math.min(500, Math.max(1, Math.trunc(parsed)));
  }

  function applyFilters() {
    setRenderCap(RENDER_CAP_STEP);
    const filter: AuditFilter = {
      limit: parseLimit(String(limit)),
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

  const columns = useMemo(
    () => [
      columnHelper.display({
        id: "select",
        size: 28,
        header: ({ table }) => (
          <input
            type="checkbox"
            className="size-4 accent-primary"
            checked={table.getIsAllRowsSelected()}
            ref={(el) => {
              if (el) el.indeterminate = table.getIsSomeRowsSelected() && !table.getIsAllRowsSelected();
            }}
            onChange={table.getToggleAllRowsSelectedHandler()}
          />
        ),
        cell: ({ row }) => (
          <input
            type="checkbox"
            className="size-4 accent-primary"
            checked={row.getIsSelected()}
            onChange={row.getToggleSelectedHandler()}
          />
        ),
        enableSorting: false,
        enableHiding: false,
      }),
      columnHelper.accessor("ts", {
        header: t("time"),
        cell: (info) => (
          <span className="font-mono text-xs text-faint">
            {new Date(info.getValue()).toLocaleString()}
          </span>
        ),
      }),
      columnHelper.accessor("host", {
        header: t("host"),
        cell: (info) => <strong className="truncate font-semibold">{info.getValue()}</strong>,
      }),
      columnHelper.accessor("command", {
        header: t("Command"),
        enableSorting: false,
        cell: (info) => (
          <code
            className={cn(
              "block truncate font-mono text-sm",
              COMMAND_TINT[info.row.original.risk_level ?? "low"]
            )}
          >
            {info.getValue()}
          </code>
        ),
      }),
      columnHelper.accessor("duration_ms", {
        id: "duration_ms",
        header: t("Duration"),
        cell: (info) => {
          const exit = info.row.original.exit_code;
          const result: "ok" | "fail" | "none" =
            exit === null ? "none" : exit === 0 ? "ok" : "fail";
          return (
            <span className="flex items-center gap-1.5 font-mono text-xs not-italic text-faint">
              <span className={cn("size-1.5 shrink-0 rounded-full", RESULT_DOT[result])} aria-hidden />
              exit={exit ?? "signal"} {info.getValue()}ms
            </span>
          );
        },
      }),
      columnHelper.accessor("risk_level", {
        header: t("Risk level"),
        cell: (info) => <RiskBadge level={info.getValue() ?? "low"} hideLow />,
      }),
      // v3: replay raises the terminal drawer and re-runs the command (§3.4).
      ...(onReplay
        ? [
            columnHelper.display({
              id: "replay",
              size: 40,
              header: () => null,
              cell: ({ row }) => (
                <IconButton
                  className="size-6"
                  title={t("Replay")}
                  onClick={() => onReplay(row.original)}
                >
                  <RotateCcw size={13} />
                </IconButton>
              ),
              enableSorting: false,
              enableHiding: false,
            }),
          ]
        : []),
    ],
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [t, onReplay]
  );

  const table = useReactTable({
    data: audit,
    columns,
    state: { sorting, rowSelection },
    onSortingChange: setSorting,
    onRowSelectionChange: setRowSelection,
    getRowId: (row) => row.id,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    enableRowSelection: true,
  });

  const rows = table.getRowModel().rows;
  const selectedIds = Object.keys(rowSelection).filter((id) => rowSelection[id]);

  // v3: risk count chips over the currently loaded entries.
  const riskCounts = useMemo(() => {
    const counts: Record<RiskLevel | "all", number> = {
      all: audit.length,
      low: 0,
      medium: 0,
      high: 0,
      blocked: 0,
    };
    for (const entry of audit) counts[entry.risk_level] += 1;
    return counts;
  }, [audit]);

  function selectRisk(level: RiskLevel | "") {
    setRiskFilter(level);
    setRenderCap(RENDER_CAP_STEP);
    onRefresh({
      limit: parseLimit(String(limit)),
      host: hostFilter.trim() || null,
      risk_level: level || null,
    });
  }

  function copySelectedAsJson() {
    const selected = audit.filter((entry) => rowSelection[entry.id]);
    navigator.clipboard
      .writeText(JSON.stringify(selected, null, 2))
      .then(() => showToast("success", t("Copied {count} entries to clipboard", { count: selected.length })))
      .catch((err) => showToast("error", String(err)));
  }

  return (
    <Card className="space-y-3 p-4">
      <div className="flex items-center gap-2 font-semibold">
        <History size={16} className="text-muted-foreground" />
        {t("Audit")}
        <ColumnVisibilityMenu table={table} label={t("Toggle columns")} />
        <IconButton
          onClick={() => setShowFilters(!showFilters)}
          title={t("Toggle filters")}
        >
          <SlidersHorizontal size={15} />
        </IconButton>
        <IconButton onClick={() => onRefresh()} title={t("Refresh audit")}>
          <RefreshCw size={15} />
        </IconButton>
      </div>

      {/* v3: risk filter chips — selected = solid risk color, idle = outline */}
      <div className="flex flex-wrap items-center gap-1.5">
        <button
          type="button"
          onClick={() => selectRisk("")}
          className={cn(
            "rounded-full border px-2.5 py-0.5 font-mono text-[10.5px] font-bold uppercase tracking-wide transition-colors",
            riskFilter === ""
              ? "border-transparent bg-foreground text-background"
              : "border-border text-muted-foreground hover:border-edge-strong hover:text-foreground"
          )}
        >
          {t("all")} {riskCounts.all}
        </button>
        {(Object.keys(CHIP_ACTIVE) as RiskLevel[]).map((level) => (
          <button
            key={level}
            type="button"
            onClick={() => selectRisk(level)}
            className={cn(
              "rounded-full border px-2.5 py-0.5 font-mono text-[10.5px] font-bold uppercase tracking-wide transition-colors",
              riskFilter === level ? CHIP_ACTIVE[level] : CHIP_IDLE[level]
            )}
          >
            {t(level)} {riskCounts[level]}
          </button>
        ))}
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
              onChange={(e) => setLimit(parseLimit(e.target.value))}
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

      {selectedIds.length > 0 && (
        <div className="flex items-center gap-2 rounded-lg border border-border bg-muted/40 px-3 py-2">
          <span className="text-xs text-muted-foreground">
            {t("{count} selected", { count: selectedIds.length })}
          </span>
          <Button variant="secondary" size="sm" className="ml-auto" onClick={copySelectedAsJson}>
            <Clipboard size={13} />
            {t("Copy selected as JSON")}
          </Button>
        </div>
      )}

      {audit.length === 0 ? (
        <EmptyState icon={History} title={t("No commands executed yet")} />
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full min-w-[640px] border-collapse text-sm">
            <thead>
              {table.getHeaderGroups().map((headerGroup) => (
                <tr key={headerGroup.id} className="border-b border-border text-left">
                  {headerGroup.headers.map((header) => (
                    <th key={header.id} className="px-2 py-1.5 font-medium text-muted-foreground">
                      {header.isPlaceholder ? null : header.column.getCanSort() ? (
                        <button
                          type="button"
                          className="flex items-center gap-1 hover:text-foreground"
                          onClick={header.column.getToggleSortingHandler()}
                        >
                          {flexRender(header.column.columnDef.header, header.getContext())}
                          <SortIcon direction={header.column.getIsSorted()} />
                        </button>
                      ) : (
                        flexRender(header.column.columnDef.header, header.getContext())
                      )}
                    </th>
                  ))}
                </tr>
              ))}
            </thead>
            <tbody>
              {rows.slice(0, renderCap).map((row) => (
                <tr
                  key={row.id}
                  className={cn(
                    "border-t border-border",
                    row.original.risk_level === "high" && "bg-risk-high/5",
                    row.original.risk_level === "blocked" && "bg-risk-high/5"
                  )}
                >
                  {row.getVisibleCells().map((cell) => (
                    <td key={cell.id} className="px-2 py-2 align-middle">
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
          {rows.length > renderCap && (
            <button
              type="button"
              onClick={() => setRenderCap((c) => c + RENDER_CAP_STEP)}
              className="w-full border-t border-border pt-2 text-center text-xs text-muted-foreground hover:text-foreground"
            >
              {t("Show more ({count} hidden)", { count: rows.length - renderCap })}
            </button>
          )}
        </div>
      )}
    </Card>
  );
}
