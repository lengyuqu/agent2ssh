import { Plug, PlugZap, RefreshCw, Server, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import type { ConnectionStatus, HostProfile } from "../types";

type Props = {
  hosts: HostProfile[];
  selectedHost: string;
  connectionStatuses: ConnectionStatus[];
  onSelect: (name: string) => void;
  onRemove: (name: string) => void;
  onRefresh: () => void;
  onConnect: (name: string) => void;
  onDisconnect: (name: string) => void;
};

export default function HostList({
  hosts,
  selectedHost,
  connectionStatuses,
  onSelect,
  onRemove,
  onRefresh,
  onConnect,
  onDisconnect,
}: Props) {
  const [confirmTarget, setConfirmTarget] = useState<string | null>(null);
  const [filters, setFilters] = useState({ env: "", role: "", owner: "", tag: "" });

  const filteredHosts = useMemo(
    () =>
      hosts.filter((host) => {
        if (!matchesFilter(host.env, filters.env)) return false;
        if (!matchesFilter(host.role, filters.role)) return false;
        if (!matchesFilter(host.owner, filters.owner)) return false;
        const tag = filters.tag.trim().toLowerCase();
        if (tag && !(host.tags ?? []).some((item) => item.trim().toLowerCase() === tag)) {
          return false;
        }
        return true;
      }),
    [hosts, filters]
  );

  function isConnected(name: string): boolean {
    return connectionStatuses.some((s) => s.host === name && s.connected);
  }

  return (
    <section className="panel">
      <div className="panel-title">
        <Server size={16} />
        Hosts
        <button
          className="icon-button"
          onClick={onRefresh}
          title="Refresh hosts"
        >
          <RefreshCw size={15} />
        </button>
      </div>
      <div className="host-filters">
        <input
          value={filters.env}
          onChange={(e) => setFilters({ ...filters, env: e.target.value })}
          placeholder="env"
        />
        <input
          value={filters.role}
          onChange={(e) => setFilters({ ...filters, role: e.target.value })}
          placeholder="role"
        />
        <input
          value={filters.owner}
          onChange={(e) => setFilters({ ...filters, owner: e.target.value })}
          placeholder="owner"
        />
        <input
          value={filters.tag}
          onChange={(e) => setFilters({ ...filters, tag: e.target.value })}
          placeholder="tag"
        />
      </div>
      <div className="host-list">
        {filteredHosts.map((host) => {
          const connected = isConnected(host.name);
          return (
            <div
              key={host.name}
              className={`host-row${host.name === selectedHost ? " active" : ""}`}
            >
              <button
                className="host"
                onClick={() => onSelect(host.name)}
              >
                <strong>
                  <span
                    className={`status-dot ${connected ? "status-connected" : "status-disconnected"}`}
                    title={connected ? "Connected" : "Disconnected"}
                  />
                  {host.name}
                </strong>
                <span>
                  {host.user ? `${host.user}@` : ""}
                  {host.host}:{host.port ?? 22}
                  {host.jump_host && ` via ${host.jump_host}`}
                </span>
                {host.tags && host.tags.length > 0 && (
                  <span className="host-tags">
                    {host.tags.map((tag) => (
                      <span key={tag} className="tag-badge">{tag}</span>
                    ))}
                  </span>
                )}
                {(host.env || host.role || host.owner) && (
                  <span className="host-meta">
                    {host.env && <span>env={host.env}</span>}
                    {host.role && <span>role={host.role}</span>}
                    {host.owner && <span>owner={host.owner}</span>}
                  </span>
                )}
              </button>
              <button
                className="icon-button host-connect"
                title={connected ? `Disconnect ${host.name}` : `Connect ${host.name}`}
                onClick={() =>
                  connected ? onDisconnect(host.name) : onConnect(host.name)
                }
              >
                {connected ? <PlugZap size={14} /> : <Plug size={14} />}
              </button>
              <button
                className="icon-button host-delete"
                title={`Remove ${host.name}`}
                onClick={() => setConfirmTarget(host.name)}
              >
                <Trash2 size={14} />
              </button>
            </div>
          );
        })}
        {hosts.length === 0 && (
          <div className="empty">No hosts configured</div>
        )}
        {hosts.length > 0 && filteredHosts.length === 0 && (
          <div className="empty">No hosts match filters</div>
        )}
      </div>

      {confirmTarget && (
        <div className="confirm-overlay" onClick={() => setConfirmTarget(null)}>
          <div className="confirm-dialog" onClick={(e) => e.stopPropagation()}>
            <p>
              Remove host <strong>{confirmTarget}</strong>?
            </p>
            <p className="confirm-hint">
              Any open sessions or forwards to this host will become orphaned.
            </p>
            <div className="confirm-actions">
              <button className="secondary" onClick={() => setConfirmTarget(null)}>
                Cancel
              </button>
              <button
                className="primary confirm-danger"
                onClick={() => {
                  onRemove(confirmTarget);
                  setConfirmTarget(null);
                }}
              >
                Remove
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}

function matchesFilter(value: string | null | undefined, filter: string): boolean {
  const normalized = filter.trim().toLowerCase();
  if (!normalized) return true;
  return (value ?? "").trim().toLowerCase() === normalized;
}
