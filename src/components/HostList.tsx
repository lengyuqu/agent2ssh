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
import { useI18n } from "../i18n";
import type { ConnectionStatus, HostGroup, HostProfile } from "../types";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { Dialog } from "./ui/dialog";
import { IconButton } from "./ui/icon-button";
import { Input } from "./ui/input";
import { cn } from "../lib/utils";

type Props = {
  hosts: HostProfile[];
  groups: HostGroup[];
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
  onRefresh: () => void;
  onConnect: (name: string) => void;
  onDisconnect: (name: string) => void;
};

const rowActionCls =
  "flex items-center justify-center border-l border-border px-2.5 text-muted-foreground transition-colors hover:bg-muted";

export default function HostList({
  hosts,
  groups,
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
  onRefresh,
  onConnect,
  onDisconnect,
}: Props) {
  const { t } = useI18n();
  const [confirmTarget, setConfirmTarget] = useState<string | null>(null);
  const [confirmGroupTarget, setConfirmGroupTarget] = useState<string | null>(null);
  const [newGroupName, setNewGroupName] = useState("");
  const [editingGroupId, setEditingGroupId] = useState<string | null>(null);
  const [editingGroupName, setEditingGroupName] = useState("");
  const [filters, setFilters] = useState({ env: "", role: "", owner: "", tag: "" });

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

  function isConnected(name: string): boolean {
    return connectionStatuses.some((s) => s.host === name && s.connected);
  }

  return (
    <Card className="space-y-4 p-4">
      <div className="flex items-center gap-2 font-semibold">
        <Server size={16} className="text-muted-foreground" />
        {t("Hosts")}
        <Badge variant="secondary" className="ml-1 font-medium">
          {t("{shown} of {total} hosts", { shown: filteredHosts.length, total: hosts.length })}
        </Badge>
        <IconButton className="ml-auto" onClick={onRefresh} title={t("Refresh hosts")}>
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

      <div className="grid max-h-[430px] gap-2 overflow-auto pr-0.5">
        {filteredHosts.map((host) => {
          const connected = isConnected(host.name);
          return (
            <div
              key={host.name}
              className={cn(
                "grid grid-cols-[minmax(0,1fr)_auto_auto_auto] items-stretch overflow-hidden rounded-lg border transition-colors",
                host.name === selectedHost
                  ? "border-primary/40 bg-primary/5"
                  : "border-border bg-card hover:border-border/80"
              )}
            >
              <button className="min-w-0 px-3 py-2.5 text-left" onClick={() => onSelect(host.name)}>
                <strong className="flex items-center gap-2 font-semibold">
                  <span
                    className={cn(
                      "size-2 shrink-0 rounded-full",
                      connected ? "bg-success" : "bg-muted-foreground/40"
                    )}
                    title={connected ? t("Connected") : t("Disconnected")}
                  />
                  <span className="truncate">{host.name}</span>
                </strong>
                <span className="mt-1 block break-all text-xs text-muted-foreground">
                  {host.user ? `${host.user}@` : ""}
                  {host.host}:{host.port ?? 22}
                  {host.jump_host && ` via ${host.jump_host}`}
                </span>
                {host.tags && host.tags.length > 0 && (
                  <span className="mt-1.5 flex flex-wrap gap-1">
                    {host.tags.map((tag) => (
                      <span
                        key={tag}
                        className="rounded bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary"
                      >
                        {tag}
                      </span>
                    ))}
                  </span>
                )}
                {(host.env || host.role || host.owner) && (
                  <span className="mt-1 flex flex-wrap gap-1.5 text-[10px] text-muted-foreground">
                    {host.env && <span>env={host.env}</span>}
                    {host.role && <span>role={host.role}</span>}
                    {host.owner && <span>owner={host.owner}</span>}
                  </span>
                )}
              </button>
              <button
                className={cn(rowActionCls, "hover:text-foreground")}
                title={t("Edit {name}", { name: host.name })}
                onClick={() => onEdit(host)}
              >
                <Edit3 size={14} />
              </button>
              <button
                className={cn(rowActionCls, "hover:text-success")}
                title={
                  connected
                    ? t("Disconnect {name}", { name: host.name })
                    : t("Connect {name}", { name: host.name })
                }
                onClick={() => (connected ? onDisconnect(host.name) : onConnect(host.name))}
              >
                {connected ? <PlugZap size={14} /> : <Plug size={14} />}
              </button>
              <button
                className={cn(rowActionCls, "hover:text-destructive")}
                title={t("Remove {name}", { name: host.name })}
                onClick={() => setConfirmTarget(host.name)}
              >
                <Trash2 size={14} />
              </button>
            </div>
          );
        })}
        {hosts.length === 0 && (
          <div className="px-3 py-3 text-sm text-muted-foreground">
            {t("No hosts configured")}
          </div>
        )}
        {hosts.length > 0 && filteredHosts.length === 0 && (
          <div className="px-3 py-3 text-sm text-muted-foreground">
            {t("No hosts match filters")}
          </div>
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
