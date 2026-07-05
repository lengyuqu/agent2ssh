import { Clipboard, History, RefreshCw, SlidersHorizontal } from "lucide-react";
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

// J3: cap mounted rows so a large `limit` query never renders thousands of nodes.
const RENDER_CAP_STEP = 200;

type Props = {
  audit: AuditEntry[];
  onRefresh: (filter?: AuditFilter) => void | Promise<void>;
};

const columnHelper = createColumnHelper<AuditEntry>();

export default function AuditPanel({ audit, onRefresh }: Props) {
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
          <span className="text-xs text-muted-foreground">
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
          <code className="block truncate font-mono text-sm">{info.getValue()}</code>
        ),
      }),
      columnHelper.accessor("duration_ms", {
        id: "duration_ms",
        header: t("Duration"),
        cell: (info) => (
          <em className="text-xs not-italic text-muted-foreground">
            exit={info.row.original.exit_code ?? "signal"} {info.getValue()}ms
          </em>
        ),
      }),
      columnHelper.accessor("risk_level", {
        header: t("Risk level"),
        cell: (info) => <RiskBadge level={info.getValue() ?? "low"} hideLow />,
      }),
    ],
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [t]
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
                    row.original.risk_level === "high" && "bg-warning/10"
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
