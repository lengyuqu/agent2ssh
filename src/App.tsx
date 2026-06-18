import {
  Activity,
  ArrowLeftRight,
  Bot,
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
import { api } from "./api";
import AddHostForm from "./components/AddHostForm";
import ApprovalDialog from "./components/ApprovalDialog";
import AuditPanel from "./components/AuditPanel";
import ExecPanel from "./components/ExecPanel";
import ForwardPanel from "./components/ForwardPanel";
import HostList from "./components/HostList";
import KeysPanel from "./components/KeysPanel";
import LiveActivityPanel from "./components/LiveActivityPanel";
import McpAgentsPanel from "./components/McpAgentsPanel";
import MultiExecPanel from "./components/MultiExecPanel";
import PingPanel from "./components/PingPanel";
import PlaybooksPanel from "./components/PlaybooksPanel";
import SFTPPanel from "./components/SFTPPanel";
import TerminalPanel from "./components/TerminalPanel";
import SettingsMenu from "./components/SettingsMenu";
import SetupWizard from "./components/SetupWizard";
import { useI18n } from "./i18n";
import { cn } from "./lib/utils";
import type { ApprovalRequest, AuditEntry, AuditFilter, ConnectionStatus, DaemonHealth, ExecutionGateStatus, HostGroup, HostProfile } from "./types";

const APPROVAL_POLL_MS = 2000;
const DAEMON_START_RECHECK_MS = 700;

const MODULES = [
  { id: "hosts", label: "Host Management", icon: Server },
  { id: "terminal", label: "Terminal", icon: Terminal },
  { id: "execute", label: "Execution", icon: Play },
  { id: "files-sessions", label: "Files", icon: FolderOpen },
  { id: "tunnels", label: "Tunnels", icon: ArrowLeftRight },
  { id: "activity", label: "Activity", icon: Activity },
  { id: "mcp-agents", label: "MCP Agents", icon: Bot },
  { id: "keys", label: "Keys", icon: Key },
  { id: "playbooks", label: "Playbooks", icon: BookOpen },
  { id: "audit", label: "Audit", icon: History },
] as const;

export default function App() {
  const { t } = useI18n();
  const [hosts, setHosts] = useState<HostProfile[]>([]);
  const [groups, setGroups] = useState<HostGroup[]>([{ id: "default", name: "Default" }]);
  const [selectedHost, setSelectedHost] = useState("");
  const [selectedGroup, setSelectedGroup] = useState("default");
  const [editingHost, setEditingHost] = useState<HostProfile | null>(null);
  const [activeModule, setActiveModule] = useState<(typeof MODULES)[number]["id"]>("hosts");
  const [audit, setAudit] = useState<AuditEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [pendingApprovals, setPendingApprovals] = useState<ApprovalRequest[]>([]);
  const [connectionStatuses, setConnectionStatuses] = useState<ConnectionStatus[]>([]);
  const [gateStatus, setGateStatus] = useState<ExecutionGateStatus | null>(null);
  const [gateBusy, setGateBusy] = useState(false);
  const [gateCheckedAt, setGateCheckedAt] = useState<number | null>(null);
  const [daemonHealth, setDaemonHealth] = useState<DaemonHealth | null>(null);
  const [daemonHealthCheckedAt, setDaemonHealthCheckedAt] = useState<number | null>(null);
  const [showWizard, setShowWizard] = useState(false);
  const [wizardDismissed, setWizardDismissed] = useState(false);

  const currentHost = useMemo(
    () => hosts.find((h) => h.name === selectedHost),
    [hosts, selectedHost]
  );

  async function refresh(filter?: AuditFilter) {
    const [hostList, groupList, auditList] = await Promise.all([
      api.listHosts(),
      api.listHostGroups(),
      api.listAudit(filter),
    ]);
    setHosts(hostList);
    setGroups(groupList.length > 0 ? groupList : [{ id: "default", name: "Default" }]);
    setAudit(auditList);
    if (!selectedHost && hostList.length > 0) setSelectedHost(hostList[0].name);
  }

  function groupIdFromName(name: string): string {
    const base = name
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9._-]+/g, "-")
      .replace(/^-+|-+$/g, "");
    return base || `group-${Date.now()}`;
  }

  async function handleCreateGroup(name: string) {
    const trimmed = name.trim();
    if (!trimmed) return;
    setError(null);
    try {
      let id = groupIdFromName(trimmed);
      let suffix = 2;
      while (groups.some((group) => group.id === id)) {
        id = `${groupIdFromName(trimmed)}-${suffix}`;
        suffix += 1;
      }
      await api.saveHostGroup({ id, name: trimmed });
      setSelectedGroup(id);
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleRenameGroup(id: string, name: string) {
    const trimmed = name.trim();
    if (!trimmed) return;
    setError(null);
    try {
      await api.saveHostGroup({ id, name: trimmed });
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleDeleteGroup(id: string) {
    if (id === "default") return;
    setError(null);
    try {
      await api.deleteHostGroup(id);
      if (selectedGroup === id) setSelectedGroup("default");
      await refresh();
    } catch (err) {
      setError(String(err));
    }
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

  const pollDaemonHealth = useCallback(async () => {
    const health = await api.getDaemonHealth();
    setDaemonHealth(health);
    setDaemonHealthCheckedAt(Date.now());
  }, []);

  const ensureLocalDaemon = useCallback(async () => {
    let health = await api.getDaemonHealth();
    if (health?.ok !== true) {
      await api
        .writeDiagnosticLog("warn", "app", "local daemon health unavailable; attempting start")
        .catch(() => {});
      try {
        await api.daemonStart();
        await new Promise((resolve) => window.setTimeout(resolve, DAEMON_START_RECHECK_MS));
        health = await api.getDaemonHealth();
      } catch (err) {
        await api
          .writeDiagnosticLog("error", "app", "failed to start local daemon", {
            error: String(err),
          })
          .catch(() => {});
        setError(t("Failed to start daemon: {error}", { error: String(err) }));
      }
    }

    setDaemonHealth(health);
    setDaemonHealthCheckedAt(Date.now());
    const status = await api.getGateStatus();
    setGateStatus(status);
    setGateCheckedAt(Date.now());
  }, [t]);

  useEffect(() => {
    const id = setInterval(pollDaemonHealth, 10000);
    pollDaemonHealth();
    return () => clearInterval(id);
  }, [pollDaemonHealth]);

  useEffect(() => {
    ensureLocalDaemon().catch((err) => setError(String(err)));
  }, [ensureLocalDaemon]);

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
      <main className="flex min-h-screen flex-col items-center justify-center gap-4 bg-background text-muted-foreground">
        <Loader2 size={32} className="animate-spin" />
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
    <main className="flex min-h-screen bg-background text-foreground max-lg:flex-col">
      {/* Approval dialog overlay (Fix-1) */}
      {currentApproval && (
        <ApprovalDialog
          command={currentApproval.command}
          riskLevel={currentApproval.risk_level}
          onConfirm={() => handleApprove(currentApproval)}
          onCancel={() => handleReject(currentApproval)}
        />
      )}

      <aside className="flex shrink-0 flex-col gap-5 bg-sidebar p-5 text-sidebar-foreground lg:w-[280px] max-lg:w-full">
        <div className="flex items-center gap-3">
          <div className="flex size-9 items-center justify-center rounded-lg bg-sidebar-accent/15 text-sidebar-accent">
            <Terminal size={20} />
          </div>
          <div className="min-w-0">
            <h1 className="text-lg font-bold leading-tight">Agent2SSH</h1>
            <span className="block text-xs text-sidebar-foreground/55">
              {t("Local SSH capability layer")}
            </span>
          </div>
        </div>
        <nav
          className="flex flex-col gap-1 max-lg:flex-row max-lg:flex-wrap"
          aria-label={t("Modules")}
        >
          <div className="px-2 pb-1 text-[11px] font-bold uppercase tracking-wider text-sidebar-foreground/45 max-lg:hidden">
            {t("Modules")}
          </div>
          {MODULES.map(({ id, label, icon: Icon }) => {
            const active = activeModule === id;
            return (
              <button
                key={id}
                type="button"
                className={cn(
                  "flex items-center gap-2.5 rounded-lg px-3 py-2 text-left text-sm font-medium transition-colors",
                  active
                    ? "bg-sidebar-accent text-white shadow-sm"
                    : "text-sidebar-foreground/80 hover:bg-white/5 hover:text-sidebar-foreground"
                )}
                onClick={() => openModule(id)}
              >
                <Icon size={15} className={active ? undefined : "opacity-70"} />
                {t(label)}
              </button>
            );
          })}
        </nav>
        <PingPanel hosts={hosts} />
        {pendingApprovals.length > 0 && (
          <div className="flex items-center gap-2 rounded-lg border border-warning/30 bg-warning/10 px-3 py-2 text-sm font-medium text-warning">
            <span className="size-2 animate-pulse rounded-full bg-warning" />
            {pendingApprovals.length}{" "}
            {t(pendingApprovals.length > 1 ? "pending approvals" : "pending approval")}
          </div>
        )}
      </aside>

      <section className="flex min-w-0 flex-1 flex-col gap-5 p-6">
        <header className="flex items-start justify-between gap-4 max-md:flex-col">
          <div className="min-w-0">
            <h2 className="flex items-center gap-2 text-xl font-bold">
              <ActiveModuleIcon size={20} className="text-muted-foreground" />
              {t(activeModuleMeta.label)}
            </h2>
            <p className="mt-1 text-sm text-muted-foreground">
              {activeModule === "hosts" && currentHost
                ? `${currentHost.user ? `${currentHost.user}@` : ""}${currentHost.host}:${currentHost.port ?? 22}${currentHost.jump_host ? ` via ${currentHost.jump_host}` : ""}`
                : hosts.length > 0
                  ? t("{count} hosts configured", { count: hosts.length })
                : t("Add a host to start issuing SSH commands")}
            </p>
          </div>
          <div className="flex flex-wrap items-center justify-end gap-2 max-md:justify-start">
            {pendingApprovals.length > 0 && (
              <div className="inline-flex items-center gap-2 rounded-lg border border-warning/30 bg-warning/10 px-3 py-2 text-sm font-medium text-warning">
                <span className="size-2 animate-pulse rounded-full bg-warning" />
                {pendingApprovals.length}{" "}
                {t(pendingApprovals.length > 1 ? "pending approvals" : "pending approval")}
              </div>
            )}
            <div
              className={cn(
                "inline-flex items-center gap-2 rounded-lg border px-3 py-2 text-sm font-medium",
                gateStatus === null
                  ? "border-border bg-muted text-muted-foreground"
                  : gateStatus.mode === "paused"
                    ? "border-destructive/30 bg-destructive/10 text-destructive"
                    : "border-success/30 bg-success/10 text-success"
              )}
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
              daemonHealth={daemonHealth}
              daemonHealthCheckedAt={daemonHealthCheckedAt}
              onGateToggle={handleGateToggle}
              onGateRefresh={pollGateStatus}
              onDaemonHealthRefresh={pollDaemonHealth}
              onImportConfig={handleImportConfig}
              onOpenSetup={() => {
                setWizardDismissed(false);
                setShowWizard(true);
              }}
            />
          </div>
        </header>

        {error && (
          <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm text-destructive">
            {error}
          </div>
        )}

        <section className="module-page">
          {activeModule === "hosts" && (
            <div className="grid grid-cols-[minmax(0,1.4fr)_minmax(340px,0.6fr)] items-start gap-[18px] max-lg:grid-cols-1">
              <HostList
                hosts={hosts}
                groups={groups}
                selectedHost={selectedHost}
                selectedGroup={selectedGroup}
                connectionStatuses={connectionStatuses}
                onSelect={setSelectedHost}
                onGroupSelect={setSelectedGroup}
                onCreateGroup={handleCreateGroup}
                onRenameGroup={handleRenameGroup}
                onDeleteGroup={handleDeleteGroup}
                onEdit={setEditingHost}
                onRemove={handleRemoveHost}
                onRefresh={refresh}
                onConnect={handleConnect}
                onDisconnect={handleDisconnect}
              />
              <AddHostForm
                hosts={hosts}
                groups={groups}
                initialGroup={selectedGroup}
                editingHost={editingHost}
                onCancelEdit={() => setEditingHost(null)}
                onSaved={refresh}
              />
            </div>
          )}

          {activeModule === "terminal" && (
            <TerminalPanel hosts={hosts} initialHost={selectedHost} />
          )}

          {activeModule === "execute" && (
            <div className="grid gap-[18px]">
              <ExecPanel hosts={hosts} initialHost={selectedHost} onExecComplete={refresh} />
              <MultiExecPanel hosts={hosts} onExecComplete={refresh} />
            </div>
          )}

          {activeModule === "files-sessions" && (
            <SFTPPanel hosts={hosts} initialHost={selectedHost} />
          )}

          {activeModule === "tunnels" && <ForwardPanel hosts={hosts} initialHost={selectedHost} />}

          {activeModule === "activity" && <LiveActivityPanel audit={audit} />}

          {activeModule === "mcp-agents" && <McpAgentsPanel />}

          {activeModule === "keys" && <KeysPanel />}

          {activeModule === "playbooks" && <PlaybooksPanel hosts={hosts} />}

          {activeModule === "audit" && <AuditPanel audit={audit} onRefresh={refresh} />}
        </section>
      </section>
    </main>
  );
}
