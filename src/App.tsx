import { Activity, Loader2, Terminal } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import "./styles.css";
import { api } from "./api";
import AddHostForm from "./components/AddHostForm";
import AuditPanel from "./components/AuditPanel";
import ExecPanel from "./components/ExecPanel";
import ForwardPanel from "./components/ForwardPanel";
import HostList from "./components/HostList";
import MultiExecPanel from "./components/MultiExecPanel";
import PingPanel from "./components/PingPanel";
import SFTPPanel from "./components/SFTPPanel";
import SessionPanel from "./components/SessionPanel";
import type { AuditEntry, AuditFilter, HostProfile } from "./types";

export default function App() {
  const [hosts, setHosts] = useState<HostProfile[]>([]);
  const [selectedHost, setSelectedHost] = useState("");
  const [audit, setAudit] = useState<AuditEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const currentHost = useMemo(
    () => hosts.find((h) => h.name === selectedHost),
    [hosts, selectedHost]
  );

  async function refresh(filter?: AuditFilter) {
    const [hostList, auditList] = await Promise.all([
      api.listHosts(),
      api.listAudit(filter),
    ]);
    setHosts(hostList);
    setAudit(auditList);
    if (!selectedHost && hostList.length > 0) setSelectedHost(hostList[0].name);
  }

  useEffect(() => {
    refresh()
      .catch((err) => setError(String(err)))
      .finally(() => setLoading(false));
  }, []);

  async function handleRemoveHost(name: string) {
    setError(null);
    try {
      await api.removeHost(name);
      if (selectedHost === name) setSelectedHost("");
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleImportConfig() {
    setError(null);
    try {
      await api.importSshConfig();
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  if (loading) {
    return (
      <main className="app-loading">
        <Loader2 size={32} className="spin" />
        <span>Loading Agent2SSH...</span>
      </main>
    );
  }

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <Terminal size={24} />
          <div>
            <h1>Agent2SSH</h1>
            <span>Local SSH capability layer</span>
          </div>
        </div>
        <HostList
          hosts={hosts}
          selectedHost={selectedHost}
          onSelect={setSelectedHost}
          onRemove={handleRemoveHost}
          onRefresh={refresh}
        />
        <button className="secondary import-btn" onClick={handleImportConfig}>
          Import from ~/.ssh/config
        </button>
        <PingPanel hosts={hosts} />
      </aside>

      <section className="workspace">
        <header className="topbar">
          <div>
            <h2>{currentHost?.name ?? "No host selected"}</h2>
            <p>
              {currentHost
                ? `${currentHost.user ? `${currentHost.user}@` : ""}${currentHost.host}:${currentHost.port ?? 22}${currentHost.jump_host ? ` via ${currentHost.jump_host}` : ""}`
                : "Add a host to start issuing SSH commands"}
            </p>
          </div>
          <div className="status-pill">
            <Activity size={15} />
            Local daemon embedded
          </div>
        </header>

        {error && <div className="error">{error}</div>}

        <div className="grid">
          <ExecPanel selectedHost={selectedHost} onExecComplete={refresh} />
          <AddHostForm hosts={hosts} onSaved={refresh} />
        </div>

        <MultiExecPanel hosts={hosts} onExecComplete={refresh} />

        <div className="grid grid-equal">
          <SFTPPanel selectedHost={selectedHost} />
          <SessionPanel selectedHost={selectedHost} />
        </div>

        <ForwardPanel selectedHost={selectedHost} />

        <AuditPanel audit={audit} onRefresh={refresh} />
      </section>
    </main>
  );
}
