import {
  Activity,
  ArrowLeftRight,
  BookOpen,
  FolderOpen,
  History,
  Key,
  Loader2,
  Play,
  Server,
  Terminal,
} from "lucide-react";
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
import SettingsMenu from "./components/SettingsMenu";
import SetupWizard from "./components/SetupWizard";
import { useI18n } from "./i18n";
import type { ApprovalRequest, AuditEntry, AuditFilter, ConnectionStatus, ExecutionGateStatus, HostProfile } from "./types";

const APPROVAL_POLL_MS = 2000;

const MODULES = [
  { id: "hosts", label: "Host Management", icon: Server },
  { id: "execute", label: "Execution", icon: Play },
  { id: "files-sessions", label: "Files & Sessions", icon: FolderOpen },
  { id: "tunnels", label: "Tunnels", icon: ArrowLeftRight },
  { id: "activity", label: "Activity", icon: Activity },
  { id: "keys", label: "Keys", icon: Key },
  { id: "playbooks", label: "Playbooks", icon: BookOpen },
  { id: "audit", label: "Audit", icon: History },
] as const;

export default function App() {
  const { t } = useI18n();
  const [hosts, setHosts] = useState<HostProfile[]>([]);
  const [selectedHost, setSelectedHost] = useState("");
  const [activeModule, setActiveModule] = useState<(typeof MODULES)[number]["id"]>("hosts");
  const [audit, setAudit] = useState<AuditEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [pendingApprovals, setPendingApprovals] = useState<ApprovalRequest[]>([]);
  const [connectionStatuses, setConnectionStatuses] = useState<ConnectionStatus[]>([]);
  const [gateStatus, setGateStatus] = useState<ExecutionGateStatus | null>(null);
  const [gateBusy, setGateBusy] = useState(false);
  const [gateCheckedAt, setGateCheckedAt] = useState<number | null>(null);
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
    setGateCheckedAt(Date.now());
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
      setError(t("Failed to update execution gate: {error}", { error: String(err) }));
    } finally {
      setGateBusy(false);
    }
  }

  async function handleApprove(approval: ApprovalRequest) {
    try {
      await api.approvalApprove(approval.id);
      setPendingApprovals((prev) => prev.filter((a) => a.id !== approval.id));
    } catch (err) {
      setError(t("Failed to approve: {error}", { error: String(err) }));
    }
  }

  async function handleReject(approval: ApprovalRequest) {
    try {
      await api.approvalReject(approval.id);
      setPendingApprovals((prev) => prev.filter((a) => a.id !== approval.id));
    } catch (err) {
      setError(t("Failed to reject: {error}", { error: String(err) }));
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

  function openModule(id: (typeof MODULES)[number]["id"]) {
    setActiveModule(id);
  }

  const activeModuleMeta = MODULES.find((module) => module.id === activeModule) ?? MODULES[0];
  const ActiveModuleIcon = activeModuleMeta.icon;

  async function handleConnect(name: string) {
    setError(null);
    try {
      await api.sshConnect(name);
      await pollConnections();
    } catch (err) {
      setError(t("Failed to connect: {error}", { error: String(err) }));
    }
  }

  async function handleDisconnect(name: string) {
    setError(null);
    try {
      await api.sshDisconnect(name);
      await pollConnections();
    } catch (err) {
      setError(t("Failed to disconnect: {error}", { error: String(err) }));
    }
  }

  if (loading) {
    return (
      <main className="app-loading">
        <Loader2 size={32} className="spin" />
        <span>{t("Loading Agent2SSH...")}</span>
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
            <span>{t("Local SSH capability layer")}</span>
          </div>
        </div>
        <nav className="module-nav" aria-label={t("Modules")}>
          <div className="module-nav-title">{t("Modules")}</div>
          {MODULES.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              className={`module-nav-item${activeModule === id ? " active" : ""}`}
              onClick={() => openModule(id)}
            >
              <Icon size={15} />
              {t(label)}
            </button>
          ))}
        </nav>
        <PingPanel hosts={hosts} />
        {pendingApprovals.length > 0 && (
          <div className="approval-indicator">
            <span className="approval-dot" />
            {pendingApprovals.length}{" "}
            {t(pendingApprovals.length > 1 ? "pending approvals" : "pending approval")}
          </div>
        )}
      </aside>

      <section className="workspace">
        <header className="topbar">
          <div>
            <h2>
              <ActiveModuleIcon size={20} />
              {t(activeModuleMeta.label)}
            </h2>
            <p>
              {currentHost
                ? `${currentHost.user ? `${currentHost.user}@` : ""}${currentHost.host}:${currentHost.port ?? 22}${currentHost.jump_host ? ` via ${currentHost.jump_host}` : ""}`
                : t("Add a host to start issuing SSH commands")}
            </p>
          </div>
          <div className="topbar-actions">
            {pendingApprovals.length > 0 && (
              <div className="status-pill approval-status">
                <span className="approval-dot" />
                {pendingApprovals.length}{" "}
                {t(pendingApprovals.length > 1 ? "pending approvals" : "pending approval")}
              </div>
            )}
            <div
              className={`status-pill gate-summary ${
                gateStatus === null ? "unknown" : gateStatus.mode === "paused" ? "paused" : "active"
              }`}
            >
              <Activity size={15} />
              {gateStatus === null
                ? t("Gate unavailable")
                : gateStatus.mode === "paused"
                  ? t("Gate paused")
                  : t("Gate active")}
            </div>
            <SettingsMenu
              gateStatus={gateStatus}
              gateBusy={gateBusy}
              gateCheckedAt={gateCheckedAt}
              onGateToggle={handleGateToggle}
              onGateRefresh={pollGateStatus}
              onImportConfig={handleImportConfig}
              onOpenSetup={() => {
                setWizardDismissed(false);
                setShowWizard(true);
              }}
            />
          </div>
        </header>

        {error && <div className="error">{error}</div>}

        <section className="module-page">
          {activeModule === "hosts" && (
            <div className="host-management-grid">
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
              <AddHostForm hosts={hosts} onSaved={refresh} />
            </div>
          )}

          {activeModule === "execute" && (
            <div className="module-stack">
              <ExecPanel selectedHost={selectedHost} onExecComplete={refresh} />
              <MultiExecPanel hosts={hosts} onExecComplete={refresh} />
            </div>
          )}

          {activeModule === "files-sessions" && (
            <div className="grid grid-equal">
              <SFTPPanel selectedHost={selectedHost} />
              <SessionPanel selectedHost={selectedHost} />
            </div>
          )}

          {activeModule === "tunnels" && <ForwardPanel selectedHost={selectedHost} />}

          {activeModule === "activity" && <LiveActivityPanel audit={audit} />}

          {activeModule === "keys" && <KeysPanel />}

          {activeModule === "playbooks" && <PlaybooksPanel hosts={hosts} />}

          {activeModule === "audit" && <AuditPanel audit={audit} onRefresh={refresh} />}
        </section>
      </section>
    </main>
  );
}
