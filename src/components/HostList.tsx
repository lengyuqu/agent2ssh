import { RefreshCw, Server, Trash2 } from "lucide-react";
import type { HostProfile } from "../types";

type Props = {
  hosts: HostProfile[];
  selectedHost: string;
  onSelect: (name: string) => void;
  onRemove: (name: string) => void;
  onRefresh: () => void;
};

export default function HostList({
  hosts,
  selectedHost,
  onSelect,
  onRemove,
  onRefresh,
}: Props) {
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
      <div className="host-list">
        {hosts.map((host) => (
          <div
            key={host.name}
            className={`host-row${host.name === selectedHost ? " active" : ""}`}
          >
            <button
              className="host"
              onClick={() => onSelect(host.name)}
            >
              <strong>{host.name}</strong>
              <span>
                {host.user ? `${host.user}@` : ""}
                {host.host}:{host.port ?? 22}
              </span>
            </button>
            <button
              className="icon-button host-delete"
              title={`Remove ${host.name}`}
              onClick={() => {
                if (confirm(`Remove host "${host.name}"?`)) onRemove(host.name);
              }}
            >
              <Trash2 size={14} />
            </button>
          </div>
        ))}
        {hosts.length === 0 && (
          <div className="empty">No hosts configured</div>
        )}
      </div>
    </section>
  );
}
