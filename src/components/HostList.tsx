import {
  Edit3,
  Folder,
  Plug,
  PlugZap,
  Plus,
  RefreshCw,
  Save,
  Server,
  Trash2,
  X,
} from "lucide-react";
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
import type { ConnectionStatus, HostGroup, HostProfile, ProxyProfile } from "../types";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { ColumnVisibilityMenu, SortIcon } from "./ui/data-table";
import { Dialog } from "./ui/dialog";
import { IconButton } from "./ui/icon-button";
import { Input } from "./ui/input";
import { EmptyState } from "./ui/state";
import { cn } from "../lib/utils";

type Props = {
  hosts: HostProfile[];
  groups: HostGroup[];
  proxies: ProxyProfile[];
  selectedHost: string;
  selectedGroup: string;
  connectionStatuses: ConnectionStatus[];
  onSelect: (name: string) => void;
  onGroupSelect: (id: string) => void;
  onCreateGroup: (name: string) => void | Promise<void>;
  onRenameGroup: (id: string, name: string) => void | Promise<void>;
  onDeleteGroup: (id: string) => void | Promise<void>;
  onEdit: (host: HostProfile) => void;
  onRemove: (name: string) => void;
  onBatchRemove: (names: string[]) => void;
  onRefresh: () => void;
  onConnect: (name: string) => void;
  onDisconnect: (name: string) => void;
};

type ConnState = { connected: boolean; dotClass: string; label: string };

type Row = {
  host: HostProfile;
  connected: boolean;
  connState: ConnState;
  address: string;
};

const columnHelper = createColumnHelper<Row>();

export default function HostList({
  hosts,
  groups,
  proxies,
  selectedHost,
  selectedGroup,
  connectionStatuses,
  onSelect,
  onGroupSelect,
  onCreateGroup,
  onRenameGroup,
  onDeleteGroup,
  onEdit,
  onRemove,
  onBatchRemove,
  onRefresh,
  onConnect,
  onDisconnect,
}: Props) {
  const { t } = useI18n();
  const [confirmTarget, setConfirmTarget] = useState<string | null>(null);
  const [confirmBatch, setConfirmBatch] = useState(false);
  const [confirmGroupTarget, setConfirmGroupTarget] = useState<string | null>(null);
  const [newGroupName, setNewGroupName] = useState("");
  const [editingGroupId, setEditingGroupId] = useState<string | null>(null);
  const [editingGroupName, setEditingGroupName] = useState("");
  const [filters, setFilters] = useState({ env: "", role: "", owner: "", tag: "" });
  const [sorting, setSorting] = useState<SortingState>([]);
  const [rowSelection, setRowSelection] = useState<RowSelectionState>({});

  const hostCountsByGroup = useMemo(() => {
    const counts = new Map<string, number>();
    for (const host of hosts) {
      const group = host.group || "default";
      counts.set(group, (counts.get(group) ?? 0) + 1);
    }
    return counts;
  }, [hosts]);

  const filteredHosts = useMemo(
    () =>
      hosts.filter((host) => {
        if ((host.group || "default") !== selectedGroup) return false;
        if (!matchesFilter(host.env, filters.env)) return false;
        if (!matchesFilter(host.role, filters.role)) return false;
        if (!matchesFilter(host.owner, filters.owner)) return false;
        const tag = filters.tag.trim().toLowerCase();
        if (tag && !(host.tags ?? []).some((item) => item.trim().toLowerCase() === tag)) {
          return false;
        }
        return true;
      }),
    [hosts, filters, selectedGroup]
  );

  // K5: surface the supervisor's liveness/reconnect state, not just presence.
  function connectionState(name: string): ConnState {
    const status = connectionStatuses.find((s) => s.host === name);
    if (!status || !status.connected) {
      return { connected: false, dotClass: "bg-muted-foreground/40", label: t("Disconnected") };
    }
    if (status.reconnecting) {
      return {
        connected: true,
        dotClass: "bg-warning animate-pulse",
        label: status.last_error
          ? `${t("Reconnecting")}: ${status.last_error}`
          : t("Reconnecting"),
      };
    }
    if (status.healthy === false) {
      return {
        connected: true,
        dotClass: "bg-destructive",
        label: status.last_error ? `${t("Stale")}: ${status.last_error}` : t("Stale"),
      };
    }
    return { connected: true, dotClass: "bg-success", label: t("Connected") };
  }

  function proxyLabel(proxyId?: string | null): string | null {
    if (!proxyId) return null;
    const proxy = proxies.find((item) => item.id === proxyId);
    if (!proxy) return proxyId;
    return `${proxy.name} (${proxy.protocol})`;
  }

  const rows = useMemo<Row[]>(
    () =>
      filteredHosts.map((host) => {
        const connState = connectionState(host.name);
        return {
          host,
          connected: connState.connected,
          connState,
          address: `${host.user ? `${host.user}@` : ""}${host.host}:${host.port ?? 22}`,
        };
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [filteredHosts, connectionStatuses, t]
  );

  const columns = useMemo(
    () => [
      columnHelper.display({
        id: "select",
        size: 32,
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
            onClick={(e) => e.stopPropagation()}
          />
        ),
        enableSorting: false,
        enableHiding: false,
      }),
      columnHelper.accessor((row) => row.host.name, {
        id: "name",
        header: t("Name"),
        cell: ({ row }) => (
          <span className="flex min-w-0 items-center gap-2 font-semibold">
            <span
              className={cn("size-2 shrink-0 rounded-full", row.original.connState.dotClass)}
              title={row.original.connState.label}
            />
            <span className="truncate">{row.original.host.name}</span>
          </span>
        ),
        enableHiding: false,
      }),
      columnHelper.accessor((row) => row.address, {
        id: "address",
        header: t("Host"),
        cell: ({ row }) => (
          <span className="block break-all text-xs text-muted-foreground">
            {row.original.address}
            {row.original.host.jump_host && ` via ${row.original.host.jump_host}`}
            {proxyLabel(row.original.host.proxy_id) &&
              ` · ${t("proxy")} ${proxyLabel(row.original.host.proxy_id)}`}
          </span>
        ),
      }),
      columnHelper.display({
        id: "tags",
        header: t("Tags (comma-separated)"),
        cell: ({ row }) =>
          row.original.host.tags && row.original.host.tags.length > 0 ? (
            <span className="flex flex-wrap gap-1">
              {row.original.host.tags.map((tag) => (
                <span
                  key={tag}
                  className="rounded bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary"
                >
                  {tag}
                </span>
              ))}
            </span>
          ) : null,
      }),
      columnHelper.display({
        id: "meta",
        header: t("Details"),
        cell: ({ row }) => {
          const host = row.original.host;
          if (!host.env && !host.role && !host.owner) return null;
          return (
            <span className="flex flex-wrap gap-1.5 text-[10px] text-muted-foreground">
              {host.env && <span>{t("env={value}", { value: host.env })}</span>}
              {host.role && <span>{t("role={value}", { value: host.role })}</span>}
              {host.owner && <span>{t("owner={value}", { value: host.owner })}</span>}
            </span>
          );
        },
      }),
      columnHelper.accessor((row) => row.connected, {
        id: "status",
        header: t("Connected"),
        cell: ({ row }) => (
          <span className="text-xs text-muted-foreground">{row.original.connState.label}</span>
        ),
      }),
      columnHelper.display({
        id: "actions",
        header: "",
        enableHiding: false,
        cell: ({ row }) => {
          const host = row.original.host;
          const connected = row.original.connected;
          return (
            <div className="flex items-center justify-end gap-1">
              <IconButton
                title={t("Edit {name}", { name: host.name })}
                onClick={(e) => {
                  e.stopPropagation();
                  onEdit(host);
                }}
              >
                <Edit3 size={14} />
              </IconButton>
              <IconButton
                title={
                  connected
                    ? t("Disconnect {name}", { name: host.name })
                    : t("Connect {name}", { name: host.name })
                }
                onClick={(e) => {
                  e.stopPropagation();
                  connected ? onDisconnect(host.name) : onConnect(host.name);
                }}
              >
                {connected ? <PlugZap size={14} /> : <Plug size={14} />}
              </IconButton>
              <IconButton
                variant="danger"
                title={t("Remove {name}", { name: host.name })}
                onClick={(e) => {
                  e.stopPropagation();
                  setConfirmTarget(host.name);
                }}
              >
                <Trash2 size={14} />
              </IconButton>
            </div>
          );
        },
      }),
    ],
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [t, proxies]
  );

  const table = useReactTable({
    data: rows,
    columns,
    state: { sorting, rowSelection },
    onSortingChange: setSorting,
    onRowSelectionChange: setRowSelection,
    getRowId: (row) => row.host.name,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    enableRowSelection: true,
  });

  const selectedNames = Object.keys(rowSelection).filter((name) => rowSelection[name]);

  return (
    <Card className="space-y-4 p-4">
      <div className="flex items-center gap-2 font-semibold">
        <Server size={16} className="text-muted-foreground" />
        {t("Hosts")}
        <Badge variant="secondary" className="ml-1 font-medium">
          {t("{shown} of {total} hosts", { shown: filteredHosts.length, total: hosts.length })}
        </Badge>
        <ColumnVisibilityMenu table={table} label={t("Toggle columns")} />
        <IconButton onClick={onRefresh} title={t("Refresh hosts")}>
          <RefreshCw size={15} />
        </IconButton>
      </div>

      <div className="rounded-lg border border-border bg-muted/40 p-3">
        <div className="mb-2 flex items-center gap-1.5 text-xs font-bold text-muted-foreground">
          <Folder size={14} />
          {t("Groups")}
        </div>
        <div className="grid gap-1.5">
          {groups.map((group) => {
            const isDefault = group.id === "default";
            const editing = editingGroupId === group.id;
            return (
              <div
                key={group.id}
                className="grid grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-1"
              >
                {editing ? (
                  <>
                    <Input
                      className="h-8"
                      value={editingGroupName}
                      onChange={(event) => setEditingGroupName(event.target.value)}
                      autoFocus
                    />
                    <IconButton
                      title={t("Save")}
                      onClick={() => {
                        onRenameGroup(group.id, editingGroupName);
                        setEditingGroupId(null);
                        setEditingGroupName("");
                      }}
                    >
                      <Save size={13} />
                    </IconButton>
                    <IconButton
                      title={t("Cancel")}
                      onClick={() => {
                        setEditingGroupId(null);
                        setEditingGroupName("");
                      }}
                    >
                      <X size={13} />
                    </IconButton>
                  </>
                ) : (
                  <>
                    <button
                      className={cn(
                        "flex items-center justify-between gap-2 rounded-md border px-2.5 py-1.5 text-left text-sm transition-colors",
                        selectedGroup === group.id
                          ? "border-primary/40 bg-primary/10 text-primary"
                          : "border-border bg-card hover:bg-muted"
                      )}
                      onClick={() => onGroupSelect(group.id)}
                    >
                      <span className="truncate">{group.name}</span>
                      <span className="text-xs text-muted-foreground">
                        {hostCountsByGroup.get(group.id) ?? 0}
                      </span>
                    </button>
                    <IconButton
                      title={t("Rename group")}
                      onClick={() => {
                        setEditingGroupId(group.id);
                        setEditingGroupName(group.name);
                      }}
                    >
                      <Edit3 size={13} />
                    </IconButton>
                    {!isDefault && (
                      <IconButton
                        variant="danger"
                        title={t("Delete group")}
                        onClick={() => setConfirmGroupTarget(group.id)}
                      >
                        <Trash2 size={13} />
                      </IconButton>
                    )}
                  </>
                )}
              </div>
            );
          })}
        </div>
        <div className="mt-2 grid grid-cols-[minmax(0,1fr)_auto] gap-1.5">
          <Input
            className="h-8"
            value={newGroupName}
            onChange={(event) => setNewGroupName(event.target.value)}
            placeholder={t("New group")}
          />
          <Button
            variant="secondary"
            size="sm"
            onClick={() => {
              onCreateGroup(newGroupName);
              setNewGroupName("");
            }}
          >
            <Plus size={13} />
            {t("Add")}
          </Button>
        </div>
      </div>

      <div className="grid grid-cols-4 gap-2 max-sm:grid-cols-2">
        <Input
          className="h-8"
          value={filters.env}
          onChange={(e) => setFilters({ ...filters, env: e.target.value })}
          placeholder={t("env")}
        />
        <Input
          className="h-8"
          value={filters.role}
          onChange={(e) => setFilters({ ...filters, role: e.target.value })}
          placeholder={t("role")}
        />
        <Input
          className="h-8"
          value={filters.owner}
          onChange={(e) => setFilters({ ...filters, owner: e.target.value })}
          placeholder={t("owner")}
        />
        <Input
          className="h-8"
          value={filters.tag}
          onChange={(e) => setFilters({ ...filters, tag: e.target.value })}
          placeholder={t("tag")}
        />
      </div>

      {selectedNames.length > 0 && (
        <div className="flex items-center gap-2 rounded-lg border border-border bg-muted/40 px-3 py-2">
          <span className="text-xs text-muted-foreground">
            {t("{count} selected", { count: selectedNames.length })}
          </span>
          <Button
            variant="destructive"
            size="sm"
            className="ml-auto"
            onClick={() => setConfirmBatch(true)}
          >
            <Trash2 size={13} />
            {t("Remove selected")}
          </Button>
        </div>
      )}

      <div className="max-h-[430px] overflow-auto pr-0.5">
        {hosts.length === 0 ? (
          <EmptyState icon={Server} title={t("No hosts configured")} />
        ) : filteredHosts.length === 0 ? (
          <EmptyState icon={Server} title={t("No hosts match filters")} />
        ) : (
          <table className="w-full min-w-[560px] border-collapse text-sm">
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
              {table.getRowModel().rows.map((row) => (
                <tr
                  key={row.id}
                  onClick={() => onSelect(row.original.host.name)}
                  className={cn(
                    "cursor-pointer border-b border-border transition-colors last:border-b-0",
                    row.original.host.name === selectedHost ? "bg-primary/5" : "hover:bg-muted/50"
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
        )}
      </div>

      {confirmTarget && (
        <Dialog onClose={() => setConfirmTarget(null)} className="max-w-sm">
          <p className="mb-2">{t("Remove host {name}?", { name: confirmTarget })}</p>
          <p className="rounded-md bg-warning/10 px-2.5 py-2 text-sm text-warning">
            {t("Any open sessions or forwards to this host will become orphaned.")}
          </p>
          <div className="mt-4 flex justify-end gap-2.5">
            <Button variant="secondary" onClick={() => setConfirmTarget(null)}>
              {t("Cancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                onRemove(confirmTarget);
                setConfirmTarget(null);
              }}
            >
              {t("Remove")}
            </Button>
          </div>
        </Dialog>
      )}

      {confirmBatch && (
        <Dialog onClose={() => setConfirmBatch(false)} className="max-w-sm">
          <p className="mb-2">
            {t("Remove {count} selected hosts?", { count: selectedNames.length })}
          </p>
          <p className="rounded-md bg-warning/10 px-2.5 py-2 text-sm text-warning">
            {t("Any open sessions or forwards to these hosts will become orphaned.")}
          </p>
          <div className="mt-4 flex justify-end gap-2.5">
            <Button variant="secondary" onClick={() => setConfirmBatch(false)}>
              {t("Cancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                onBatchRemove(selectedNames);
                setRowSelection({});
                setConfirmBatch(false);
              }}
            >
              {t("Remove")}
            </Button>
          </div>
        </Dialog>
      )}

      {confirmGroupTarget && (
        <Dialog onClose={() => setConfirmGroupTarget(null)} className="max-w-sm">
          <p className="mb-2">
            {t("Delete group {name}?", {
              name:
                groups.find((group) => group.id === confirmGroupTarget)?.name ?? confirmGroupTarget,
            })}
          </p>
          <p className="rounded-md bg-warning/10 px-2.5 py-2 text-sm text-warning">
            {t("Hosts in this group will move to Default.")}
          </p>
          <div className="mt-4 flex justify-end gap-2.5">
            <Button variant="secondary" onClick={() => setConfirmGroupTarget(null)}>
              {t("Cancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={async () => {
                await onDeleteGroup(confirmGroupTarget);
                setConfirmGroupTarget(null);
              }}
            >
              {t("Delete")}
            </Button>
          </div>
        </Dialog>
      )}
    </Card>
  );
}

function matchesFilter(value: string | null | undefined, filter: string): boolean {
  const normalized = filter.trim().toLowerCase();
  if (!normalized) return true;
  return (value ?? "").trim().toLowerCase() === normalized;
}
