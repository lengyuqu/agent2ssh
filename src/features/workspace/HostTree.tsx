import { useMemo, useState } from "react";
import { FolderSearch, LayoutGrid } from "lucide-react";
import { useI18n } from "../../i18n";
import { cn } from "../../lib/utils";
import type { ConnectionStatus, HostGroup, HostProfile } from "../../types";

type Props = {
  hosts: HostProfile[];
  groups: HostGroup[];
  connectionStatuses: ConnectionStatus[];
  openTabs: Set<string>;
  collapsed: boolean;
  onOpenHost: (name: string) => void;
  /** Leave the workspace and return to the console (grouped module nav). */
  onOpenConsole: () => void;
};

/**
 * The workspace sidebar: the host tree (groups → hosts) instead of a module
 * list. Clicking a host opens or focuses its session tab; governance and
 * configuration modules are reachable through the management entry at the
 * bottom. The status dot mirrors the live connection poll.
 */
export default function HostTree({
  hosts,
  groups,
  connectionStatuses,
  openTabs,
  collapsed,
  onOpenHost,
  onOpenConsole,
}: Props) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");

  const connected = useMemo(
    () => new Set(connectionStatuses.filter((c) => c.connected).map((c) => c.host)),
    [connectionStatuses],
  );

  const matches = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return null;
    return new Set(
      hosts
        .filter(
          (host) =>
            host.name.toLowerCase().includes(needle) ||
            host.host.toLowerCase().includes(needle),
        )
        .map((host) => host.name),
    );
  }, [hosts, query]);

  return (
    <div className={cn("flex min-h-0 flex-1 flex-col gap-2", collapsed && "items-center")}>
      {!collapsed && (
        <div className="relative">
          <FolderSearch
            size={13}
            className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-sidebar-foreground/50"
          />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t("Search hosts")}
            className="w-full rounded-md border border-transparent bg-white/5 py-1.5 pl-7 pr-2 text-xs text-sidebar-foreground outline-none placeholder:text-sidebar-foreground/40 focus:border-edge"
          />
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto">
        {groups.map((group) => {
          const groupHosts = hosts.filter(
            (host) =>
              (host.group || "default") === group.id &&
              (!matches || matches.has(host.name)),
          );
          if (groupHosts.length === 0) return null;

          if (collapsed) {
            // Icon rail: one count button per group opens its first host.
            return (
              <button
                key={group.id}
                type="button"
                title={`${group.name} (${groupHosts.length})`}
                className="mb-1 flex size-8 items-center justify-center rounded-lg font-mono text-[10px] text-sidebar-foreground/70 hover:bg-white/5 hover:text-sidebar-foreground"
                onClick={() => onOpenHost(groupHosts[0].name)}
              >
                {groupHosts.length}
              </button>
            );
          }

          return (
            <div key={group.id} className="mb-1.5">
              <div className="px-2.5 pb-0.5 pt-1 text-[10px] font-bold uppercase tracking-wide text-sidebar-foreground/40">
                {t(group.name)}
              </div>
              <div className="flex flex-col gap-0.5">
                {groupHosts.map((host) => {
                  const isConnected = connected.has(host.name);
                  const hasTab = openTabs.has(host.name);
                  return (
                    <button
                      key={host.name}
                      type="button"
                      title={`${host.name} · ${host.user ? `${host.user}@` : ""}${host.host}`}
                      className={cn(
                        "flex items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] transition-colors",
                        hasTab
                          ? "bg-white/10 text-sidebar-foreground"
                          : "text-sidebar-foreground/80 hover:bg-white/5 hover:text-sidebar-foreground"
                      )}
                      onClick={() => onOpenHost(host.name)}
                    >
                      <span
                        className={cn(
                          "size-1.5 shrink-0 rounded-full",
                          isConnected ? "bg-success" : "bg-risk-medium/70",
                        )}
                        title={isConnected ? t("connected") : t("Disconnected")}
                      />
                      <span className="min-w-0 truncate">{host.name}</span>
                    </button>
                  );
                })}
              </div>
            </div>
          );
        })}
        {hosts.length === 0 && !collapsed && (
          <div className="px-2.5 py-2 text-xs text-sidebar-foreground/45">
            {t("Add a host to start issuing SSH commands")}
          </div>
        )}
      </div>

      <button
        type="button"
        onClick={onOpenConsole}
        title={t("Back to console")}
        className={cn(
          "flex shrink-0 items-center gap-2 rounded-lg text-[13px] text-sidebar-foreground/70 transition-colors hover:bg-white/5 hover:text-sidebar-foreground",
          collapsed ? "justify-center p-2" : "px-2.5 py-1.5"
        )}
      >
        <LayoutGrid size={14} className="shrink-0" />
        {!collapsed && <span>{t("Back to console")}</span>}
      </button>
    </div>
  );
}
