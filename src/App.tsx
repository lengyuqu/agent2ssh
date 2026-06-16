import { Activity, Loader2, PauseCircle, PlayCircle, Terminal } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import "./styles.css";
import { api } from "./api";
import AddHostForm from "./components/AddHostForm";
import ApprovalDialog from "./components/ApprovalDialog";
import AuditPanel from "./components/AuditPanel";
import ExecPanel from "./components/ExecPanel";
import ForwardPanel from "./components/ForwardPanel";
import HostList from "./components/HostList";
import KeysPanel from "./components/KeysPanel";
import LiveActivityPanel from "./components/LiveActivityPanel";
import MultiExecPanel from "./components/MultiExecPanel";
import PingPanel from "./components/PingPanel";
import PlaybooksPanel from "./components/PlaybooksPanel";
import SFTPPanel from "./components/SFTPPanel";
import SessionPanel from "./components/SessionPanel";
import SetupWizard from "./components/SetupWizard";
import type { ApprovalRequest, AuditEntry, AuditFilter, ConnectionStatus, ExecutionGateStatus, HostProfile } from "./types";

const APPROVAL_POLL_MS = 2000;

export default function App() {
  const [hosts, setHosts] = useState<HostProfile[]>([]);
  const [selectedHost, setSelectedHost] = useState("");
  const [audit, setAudit] = useState<AuditEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [pendingApprovals, setPendingApprovals] = useState<ApprovalRequest[]>([]);
  const [connectionStatuses, setConnectionStatuses] = useState<ConnectionStatus[]>([]);
  const [gateStatus, setGateStatus] = useState<ExecutionGateStatus | null>(null);
  const [gateBusy, setGateBusy] = useState(false);
  const [showWizard, setShowWizard] = useState(false);
  const [wizardDismissed, setWizardDismissed] = useState(false);

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

  // Show wizard when no hosts are configured and user hasn't dismissed it
  useEffect(() => {
    if (!loading && hosts.length === 0 && !wizardDismissed) {
      setShowWizard(true);
    }
  }, [loading, hosts.length, wizardDismissed]);

  // Fix-1: Poll daemon for pending approvals every 2 seconds
  const pollApprovals = useCallback(async () => {
    const approvals = await api.fetchApprovals();
    setPendingApprovals(approvals.filter((a) => a.status === "pending"));
  }, []);

  useEffect(() => {
    const id = setInterval(pollApprovals, APPROVAL_POLL_MS);
    pollApprovals(); // immediate first poll
    return () => clearInterval(id);
  }, [pollApprovals]);

  // Poll connection status every 5 seconds
  const pollConnections = useCallback(async () => {
    try {
      const statuses = await api.connectionStatus();
      setConnectionStatuses(statuses);
    } catch {
      // silent
    }
  }, []);

  useEffect(() => {
    const id = setInterval(pollConnections, 5000);
    pollConnections(); // immediate first poll
    return () => clearInterval(id);
  }, [pollConnections]);

  const pollGateStatus = useCallback(async () => {
    const status = await api.getGateStatus();
    setGateStatus(status);
  }, []);

  useEffect(() => {
    const id = setInterval(pollGateStatus, 5000);
    pollGateStatus();
    return () => clearInterval(id);
  }, [pollGateStatus]);

  async function handleGateToggle() {
    setError(null);
    setGateBusy(true);
    try {
      const status =
        gateStatus?.mode === "paused"
          ? await api.resumeGate("desktop resume")
          : await api.pauseGate("desktop emergency stop");
      setGateStatus(status);
    } catch (err) {
      setError(`Failed to update execution gate: ${err}`);
    } finally {
      setGateBusy(false);
    }
  }

  async function handleApprove(approval: ApprovalRequest) {
    try {
      await api.approvalApprove(approval.id);
      setPendingApprovals((prev) => prev.filter((a) => a.id !== approval.id));
    } catch (err) {
      setError(`Failed to approve: ${err}`);
    }
  }

  async function handleReject(approval: ApprovalRequest) {
    try {
      await api.approvalReject(approval.id);
      setPendingApprovals((prev) => prev.filter((a) => a.id !== approval.id));
    } catch (err) {
      setError(`Failed to reject: ${err}`);
    }
  }

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

  async function handleConnect(name: string) {
    setError(null);
    try {
      await api.sshConnect(name);
      await pollConnections();
    } catch (err) {
      setError(`Failed to connect: ${err}`);
    }
  }

  async function handleDisconnect(name: string) {
    setError(null);
    try {
      await api.sshDisconnect(name);
      await pollConnections();
    } catch (err) {
      setError(`Failed to disconnect: ${err}`);
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

  const currentApproval = pendingApprovals[0] ?? null;

  // Render setup wizard overlay when active
  if (showWizard) {
    return (
      <SetupWizard
        onComplete={() => {
          setShowWizard(false);
          setWizardDismissed(true);
          refresh().catch((err) => setError(String(err)));
        }}
        onSkip={() => {
          setShowWizard(false);
          setWizardDismissed(true);
        }}
      />
    );
  }

  return (
    <main className="app-shell">
      {/* Approval dialog overlay (Fix-1) */}
      {currentApproval && (
        <ApprovalDialog
          command={currentApproval.command}
          riskLevel={currentApproval.risk_level}
          onConfirm={() => handleApprove(currentApproval)}
          onCancel={() => handleReject(currentApproval)}
        />
      )}

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
          connectionStatuses={connectionStatuses}
          onSelect={setSelectedHost}
          onRemove={handleRemoveHost}
          onRefresh={refresh}
          onConnect={handleConnect}
          onDisconnect={handleDisconnect}
        />
        <button className="secondary import-btn" onClick={handleImportConfig}>
          Import from ~/.ssh/config
        </button>
        <PingPanel hosts={hosts} />
        {pendingApprovals.length > 0 && (
          <div className="approval-indicator">
            <span className="approval-dot" />
            {pendingApprovals.length} pending approval{pendingApprovals.length > 1 ? "s" : ""}
          </div>
        )}
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
          <div className="topbar-actions">
            <div className={`gate-pill ${gateStatus?.mode === "paused" ? "paused" : "active"}`}>
              <Activity size={15} />
              {gateStatus?.mode === "paused" ? "Gate paused" : "Gate active"}
            </div>
            <button
              type="button"
              className={`gate-action ${gateStatus?.mode === "paused" ? "resume" : "pause"}`}
              onClick={handleGateToggle}
              disabled={gateBusy || gateStatus === null}
              title={gateStatus?.mode === "paused" ? "Resume execution gate" : "Pause execution gate"}
            >
              {gateStatus?.mode === "paused" ? <PlayCircle size={16} /> : <PauseCircle size={16} />}
              {gateStatus?.mode === "paused" ? "Resume" : "Pause"}
            </button>
            <div className="status-pill">
              <Activity size={15} />
              Local daemon embedded
            </div>
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

        <LiveActivityPanel audit={audit} />

        <KeysPanel />

        <PlaybooksPanel hosts={hosts} />

        <AuditPanel audit={audit} onRefresh={refresh} />
      </section>
    </main>
  );
}
