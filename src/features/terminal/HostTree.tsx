import { useMemo, useState } from "react";
import { Plus, Search } from "lucide-react";
import { useI18n } from "../../i18n";
import { cn } from "../../lib/utils";
import type { ConnectionStatus, HostGroup, HostProfile } from "../../types";

type Props = {
  hosts: HostProfile[];
  groups: HostGroup[];
  connectionStatuses: ConnectionStatus[];
  /** Open session count per host name — the badge that says "already connected here". */
  sessionCounts: ReadonlyMap<string, number>;
  onOpenHost: (name: string) => void;
  onNewSession: (name: string) => void;
};

/**
 * The terminal's host tree: groups → hosts, each with the live connection dot
 * and its open-session count. Clicking a host focuses its latest session or
 * opens one; the hover "+" opens an extra session to the same host. The status
 * dot mirrors the connection poll, not the terminal's own socket, so a host
 * reachable by exec still reads as connected here.
 */
export default function HostTree({
  hosts,
  groups,
  connectionStatuses,
  sessionCounts,
  onOpenHost,
  onNewSession,
}: Props) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");

  const connected = useMemo(
    () => new Set(connectionStatuses.filter((status) => status.connected).map((status) => status.host)),
    [connectionStatuses],
  );

  const matches = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return null;
    return new Set(
      hosts
        .filter(
          (host) =>
            host.name.toLowerCase().includes(needle) || host.host.toLowerCase().includes(needle),
        )
        .map((host) => host.name),
    );
  }, [hosts, query]);

  const visibleGroups = useMemo(
    () =>
      groups
        .map((group) => ({
          group,
          hosts: hosts.filter(
            (host) =>
              (host.group || "default") === group.id && (!matches || matches.has(host.name)),
          ),
        }))
        .filter((entry) => entry.hosts.length > 0),
    [groups, hosts, matches],
  );

  return (
    <div className="flex min-h-0 flex-col gap-2">
      <div className="relative">
        <Search
          size={12}
          className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-muted-foreground"
        />
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={t("Search hosts")}
          aria-label={t("Search hosts")}
          className="h-7 w-full rounded-md border border-border bg-background/60 pl-7 pr-2 text-xs text-foreground outline-none placeholder:text-muted-foreground/70 focus:border-ring/60"
        />
      </div>

      <div className="max-h-[42vh] min-h-0 overflow-y-auto">
        {visibleGroups.map(({ group, hosts: groupHosts }) => (
          <div key={group.id} className="mb-1.5">
            <div className="px-1 pb-0.5 pt-1 text-[11px] font-bold uppercase tracking-wide text-muted-foreground">
              {t(group.name)}
            </div>
            <div className="flex flex-col gap-0.5">
              {groupHosts.map((host) => {
                const isConnected = connected.has(host.name);
                const count = sessionCounts.get(host.name) ?? 0;
                return (
                  <div
                    key={host.name}
                    className={cn(
                      "group flex items-center gap-1.5 rounded-md px-2 py-1 text-xs transition-colors",
                      count > 0 ? "bg-primary/10 text-foreground" : "text-foreground/85 hover:bg-muted",
                    )}
                  >
                    <button
                      type="button"
                      title={`${host.name} · ${host.user ? `${host.user}@` : ""}${host.host}`}
                      className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
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
                      {count > 0 && (
                        <span className="ml-auto shrink-0 rounded bg-primary/15 px-1 font-mono text-[10px] font-bold text-primary">
                          {count}
                        </span>
                      )}
                    </button>
                    <button
                      type="button"
                      title={t("New session to {host}", { host: host.name })}
                      aria-label={t("New session to {host}", { host: host.name })}
                      className="shrink-0 rounded p-0.5 text-muted-foreground opacity-0 transition-opacity hover:bg-foreground/10 hover:text-foreground focus-visible:opacity-100 group-hover:opacity-100"
                      onClick={() => onNewSession(host.name)}
                    >
                      <Plus size={11} />
                    </button>
                  </div>
                );
              })}
            </div>
          </div>
        ))}
        {visibleGroups.length === 0 && (
          <div className="px-1 py-2 text-xs text-muted-foreground">
            {hosts.length === 0
              ? t("Add a host to start issuing SSH commands")
              : t("No hosts match this search")}
          </div>
        )}
      </div>
    </div>
  );
}