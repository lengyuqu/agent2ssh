import {
  Activity,
  ArrowLeftRight,
  Bot,
  BookOpen,
  Camera,
  Cloud,
  FolderOpen,
  HelpCircle,
  History,
  Key,
  LayoutDashboard,
  Loader2,
  Network,
  PanelLeftClose,
  PanelLeftOpen,
  Play,
  Server,
  ShieldCheck,
  Terminal,
  Waypoints,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, reportError } from "./api";
import AddHostForm from "./components/AddHostForm";
import ApprovalDialog from "./components/ApprovalDialog";
import ApprovalTimeline from "./components/ApprovalTimeline";
import AuditCharts from "./components/AuditCharts";
import AuditPanel from "./components/AuditPanel";
import Breadcrumb from "./components/Breadcrumb";
import ConfigSnapshotsPanel from "./components/ConfigSnapshotsPanel";
import ConnectionTopology from "./components/ConnectionTopology";
import CommandPalette, { type CommandPaletteModule } from "./components/CommandPalette";
import Dashboard from "./components/Dashboard";
import ExecPanel from "./components/ExecPanel";
import Footbar from "./components/Footbar";
import ForwardPanel from "./components/ForwardPanel";
import HelpPanel from "./components/HelpPanel";
import HostList from "./components/HostList";
import KeysPanel from "./components/KeysPanel";
import LiveActivityPanel from "./components/LiveActivityPanel";
import McpAgentsPanel from "./components/McpAgentsPanel";
import MultiExecPanel from "./components/MultiExecPanel";
import NotificationCenter from "./components/NotificationCenter";
import PingPanel from "./components/PingPanel";
import PlaybooksPanel from "./components/PlaybooksPanel";
import ProxyPanel from "./components/ProxyPanel";
import SFTPPanel from "./components/SFTPPanel";
import TerminalPanel from "./components/TerminalPanel";
import SecretsUnlock from "./components/SecretsUnlock";
import SettingsMenu from "./components/SettingsMenu";
import SetupWizard from "./components/SetupWizard";
import SyncPanel from "./components/SyncPanel";
import { Button } from "./components/ui/button";
import { Dialog } from "./components/ui/dialog";
import { IconButton } from "./components/ui/icon-button";
import { useToast } from "./components/ui/toast";
import { useAgentEvents } from "./eventsBus";
import { useI18n } from "./i18n";
import { cn } from "./lib/utils";
import type { ApprovalRequest, AuditEntry, AuditFilter, ConnectionStatus, DaemonHealth, ExecutionGateStatus, HostFingerprintStatus, HostGroup, HostProfile, ProxyProfile } from "./types";

const APPROVAL_POLL_MS = 2000;
const DAEMON_START_RECHECK_MS = 700;
const WEBDAV_AUTO_SYNC_MS = 10 * 60 * 1000;

// Poll setState guard: keep the previous state object when a fresh poll
// result is structurally identical, so the 2s/5s/10s polls only re-render
// the app (and re-trigger downstream memos/effects, e.g. the topology force
// simulation) when something actually changed.
function keepIfEqual<T>(prev: T, next: T): T {
  return JSON.stringify(prev) === JSON.stringify(next) ? prev : next;
}

type ConnectionProgress = {
  host: string;
  title: string;
  message: string;
  percent: number;
  busy: boolean;
  variant?: "active" | "success" | "error";
};

const MODULES = [
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "hosts", label: "Host Management", icon: Server },
  { id: "proxies", label: "Proxies", icon: Network },
  { id: "terminal", label: "Terminal", icon: Terminal },
  { id: "execute", label: "Execution", icon: Play },
  { id: "files-sessions", label: "Files", icon: FolderOpen },
  { id: "tunnels", label: "Tunnels", icon: ArrowLeftRight },
  { id: "activity", label: "Activity", icon: Activity },
  { id: "mcp-agents", label: "MCP Agents", icon: Bot },
  { id: "sync", label: "Sync", icon: Cloud },
  { id: "keys", label: "Keys", icon: Key },
  { id: "playbooks", label: "Playbooks", icon: BookOpen },
  { id: "audit", label: "Audit", icon: History },
  { id: "help", label: "Help", icon: HelpCircle },
  // V2-2/V4-3: appended (not inserted) so the existing Ctrl/⌘+1..9 module
  // shortcuts (V2-5) keep pointing at the same modules.
  { id: "approvals", label: "Approvals", icon: ShieldCheck },
  { id: "config", label: "Config Snapshots", icon: Camera },
  { id: "topology", label: "Topology", icon: Waypoints },
] as const;

// V3-4: mesh navigation, not a tab trail — each module lists a handful of the
// other modules an operator is likely to jump to from here.
const RELATED_MODULES: Partial<Record<(typeof MODULES)[number]["id"], Array<(typeof MODULES)[number]["id"]>>> = {
  hosts: ["execute", "files-sessions", "tunnels", "terminal", "audit", "topology"],
  topology: ["hosts", "tunnels"],
  proxies: ["hosts"],
  terminal: ["hosts"],
  execute: ["hosts", "audit"],
  "files-sessions": ["hosts"],
  tunnels: ["hosts"],
  activity: ["audit"],
  keys: ["hosts"],
  playbooks: ["hosts", "execute"],
  audit: ["hosts", "approvals"],
  approvals: ["execute", "audit"],
  sync: ["config"],
  config: ["sync"],
};

export default function App() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [hosts, setHosts] = useState<HostProfile[]>([]);
  const [groups, setGroups] = useState<HostGroup[]>([{ id: "default", name: "Default" }]);
  const [proxies, setProxies] = useState<ProxyProfile[]>([]);
  const [selectedHost, setSelectedHost] = useState("");
  const [selectedGroup, setSelectedGroup] = useState("default");
  const [editingHost, setEditingHost] = useState<HostProfile | null>(null);
  const [activeModule, setActiveModule] = useState<(typeof MODULES)[number]["id"]>("dashboard");
  const [audit, setAudit] = useState<AuditEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [pendingApprovals, setPendingApprovals] = useState<ApprovalRequest[]>([]);
  const [connectionStatuses, setConnectionStatuses] = useState<ConnectionStatus[]>([]);
  // K1: true when the app-managed credential store is initialized but locked,
  // gating the UI behind a master-password unlock dialog at startup.
  const [secretsLocked, setSecretsLocked] = useState(false);
  const [gateStatus, setGateStatus] = useState<ExecutionGateStatus | null>(null);
  const [gateBusy, setGateBusy] = useState(false);
  const [gateCheckedAt, setGateCheckedAt] = useState<number | null>(null);
  const [daemonHealth, setDaemonHealth] = useState<DaemonHealth | null>(null);
  const [daemonHealthCheckedAt, setDaemonHealthCheckedAt] = useState<number | null>(null);
  const [showWizard, setShowWizard] = useState(false);
  const [wizardDismissed, setWizardDismissed] = useState(false);
  const [fingerprintPrompt, setFingerprintPrompt] = useState<{
    host: string;
    status: HostFingerprintStatus;
  } | null>(null);
  const [fingerprintBusy, setFingerprintBusy] = useState(false);
  const [connectionProgress, setConnectionProgress] = useState<ConnectionProgress | null>(null);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  // V4-5: icon-only sidebar, independent of the existing max-lg full-stack
  // fallback for genuinely narrow/mobile-style widths.
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => {
    try {
      return localStorage.getItem("agent2ssh.sidebarCollapsed") === "1";
    } catch {
      return false;
    }
  });
  const webDavSyncInFlight = useRef(false);
  const pendingApprovalsRef = useRef<ApprovalRequest[]>([]);
  // V2-1: previous connection-status snapshot, used to detect connect/disconnect
  // transitions for the connection-status notification. No backend SSE event
  // exists for this today (host_connected/host_disconnected are defined but
  // never published), so it is derived from the existing 5s poll instead.
  const prevConnectedRef = useRef<Map<string, boolean> | null>(null);

  const currentHost = useMemo(
    () => hosts.find((h) => h.name === selectedHost),
    [hosts, selectedHost]
  );

  const currentHostProxy = useMemo(
    () => proxies.find((proxy) => proxy.id === currentHost?.proxy_id),
    [currentHost?.proxy_id, proxies]
  );

  const refresh = useCallback(async () => {
    const [hostList, groupList, proxyList] = await Promise.all([
      api.listHosts(),
      api.listHostGroups(),
      api.listProxies(),
    ]);
    setHosts(hostList);
    setGroups(groupList.length > 0 ? groupList : [{ id: "default", name: "Default" }]);
    setProxies(proxyList);
    setSelectedHost((current) => {
      if (current && hostList.some((host) => host.name === current)) {
        return current;
      }
      return hostList[0]?.name ?? "";
    });
  }, []);

  const refreshAudit = useCallback(async (filter?: AuditFilter) => {
    const auditList = await api.listAudit(filter);
    setAudit(auditList);
  }, []);

  const runAutoWebDavSync = useCallback(async (reason: string) => {
    if (webDavSyncInFlight.current) return;
    webDavSyncInFlight.current = true;
    try {
      const config = await api.getWebDavSyncConfig();
      if (!config.enabled || !config.url.trim() || !config.remotePath.trim()) {
        return;
      }
      await api.pushWebDavSync();
    } catch (err) {
      reportError("webdav-auto-sync", "automatic webdav sync failed", err, { reason });
    } finally {
      webDavSyncInFlight.current = false;
    }
  }, []);

  async function handleHostSaved() {
    await refresh();
    void runAutoWebDavSync("host_saved");
  }

  async function handleProxyChanged() {
    await refresh();
    void runAutoWebDavSync("proxy_changed");
  }

  function handleForwardChanged() {
    void runAutoWebDavSync("forward_changed");
  }

  function handleKeyChanged() {
    void runAutoWebDavSync("key_changed");
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
      showToast("error", String(err));
    }
  }

  async function handleRenameGroup(id: string, name: string) {
    const trimmed = name.trim();
    if (!trimmed) return;
    try {
      await api.saveHostGroup({ id, name: trimmed });
      await refresh();
    } catch (err) {
      showToast("error", String(err));
    }
  }

  async function handleDeleteGroup(id: string) {
    if (id === "default") return;
    try {
      await api.deleteHostGroup(id);
      if (selectedGroup === id) setSelectedGroup("default");
      await refresh();
    } catch (err) {
      showToast("error", String(err));
    }
  }

  useEffect(() => {
    refresh()
      .catch((err) => showToast("error", String(err)))
      .finally(() => setLoading(false));
  }, [refresh]);

  useEffect(() => {
    try {
      localStorage.setItem("agent2ssh.sidebarCollapsed", sidebarCollapsed ? "1" : "0");
    } catch {
      // ignore persistence failures
    }
  }, [sidebarCollapsed]);

  useEffect(() => {
    if (activeModule !== "audit") return;
    refreshAudit().catch((err) => showToast("error", String(err)));
  }, [activeModule, refreshAudit]);

  // K1: gate the UI behind a master-password unlock when the credential store is
  // initialized but locked, so saved SSH passwords can be decrypted.
  useEffect(() => {
    api
      .secretsStatus()
      .then((status) => setSecretsLocked(status.initialized && !status.unlocked))
      .catch(() => setSecretsLocked(false));
  }, []);

  useEffect(() => {
    const id = window.setInterval(() => {
      void runAutoWebDavSync("interval");
    }, WEBDAV_AUTO_SYNC_MS);
    return () => window.clearInterval(id);
  }, [runAutoWebDavSync]);

  // Show wizard when no hosts are configured and user hasn't dismissed it
  useEffect(() => {
    if (!loading && hosts.length === 0 && !wizardDismissed) {
      setShowWizard(true);
    }
  }, [loading, hosts.length, wizardDismissed]);

  // Fix-1: Poll daemon for pending approvals every 2 seconds
  const pollApprovals = useCallback(async () => {
    const approvals = await api.fetchApprovals();
    const pending = approvals.filter((a) => a.status === "pending");
    setPendingApprovals((prev) => keepIfEqual(prev, pending));
  }, []);

  useEffect(() => {
    const id = setInterval(pollApprovals, APPROVAL_POLL_MS);
    pollApprovals(); // immediate first poll
    return () => clearInterval(id);
  }, [pollApprovals]);

  // The interval above is the fallback; when the SSE bus is live, approval
  // events land instantly, so refresh the pending list right away.
  useAgentEvents((event) => {
    if (event.event_type === "approval_requested" || event.event_type === "approval_responded") {
      pollApprovals().catch(() => {});
    }
  });

  useEffect(() => {
    pendingApprovalsRef.current = pendingApprovals;
  }, [pendingApprovals]);

  // Poll connection status every 5 seconds
  const pollConnections = useCallback(async () => {
    try {
      const statuses = await api.connectionStatus();
      setConnectionStatuses((prev) => keepIfEqual(prev, statuses));
      const prev = prevConnectedRef.current;
      if (prev) {
        for (const status of statuses) {
          const before = prev.get(status.host);
          if (before !== undefined && before !== status.connected) {
            showToast(
              status.connected ? "success" : "warning",
              t(status.connected ? "{host} connected" : "{host} disconnected", { host: status.host })
            );
          }
        }
      }
      prevConnectedRef.current = new Map(statuses.map((status) => [status.host, status.connected]));
    } catch {
      // silent
    }
  }, [showToast, t]);

  useEffect(() => {
    const id = setInterval(pollConnections, 5000);
    pollConnections(); // immediate first poll
    return () => clearInterval(id);
  }, [pollConnections]);

  const pollGateStatus = useCallback(async () => {
    const status = await api.getGateStatus();
    setGateStatus((prev) => keepIfEqual(prev, status));
    setGateCheckedAt(Date.now());
  }, []);

  useEffect(() => {
    const id = setInterval(pollGateStatus, 5000);
    pollGateStatus();
    return () => clearInterval(id);
  }, [pollGateStatus]);

  const pollDaemonHealth = useCallback(async () => {
    const health = await api.getDaemonHealth();
    setDaemonHealth((prev) => keepIfEqual(prev, health));
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
        showToast("error", t("Failed to start daemon: {error}", { error: String(err) }));
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
    ensureLocalDaemon().catch((err) => showToast("error", String(err)));
  }, [ensureLocalDaemon]);

  // V2-5: global keyboard shortcuts. Ctrl/Cmd+K opens the command palette,
  // Ctrl/Cmd+1..9 jumps straight to a module, Ctrl/Cmd+Shift+A surfaces the
  // pending approval dialog (or reports there is none to act on).
  useEffect(() => {
    function handleGlobalKeyDown(e: KeyboardEvent) {
      const mod = e.metaKey || e.ctrlKey;
      if (!mod) return;
      const key = e.key.toLowerCase();

      if (key === "k") {
        e.preventDefault();
        setCommandPaletteOpen(true);
        return;
      }

      if (e.shiftKey && key === "a") {
        e.preventDefault();
        const approval = pendingApprovalsRef.current[0];
        if (approval) {
          document.getElementById("approval-dialog-cancel")?.focus();
        } else {
          showToast("success", t("No pending approvals"));
        }
        return;
      }

      if (!e.shiftKey && key >= "1" && key <= "9") {
        const index = Number(key) - 1;
        const module = MODULES[index];
        if (module) {
          e.preventDefault();
          setActiveModule(module.id);
        }
      }
    }
    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => window.removeEventListener("keydown", handleGlobalKeyDown);
  }, [showToast, t]);

  async function handleGateToggle() {
    setGateBusy(true);
    try {
      const status =
        gateStatus?.mode === "paused"
          ? await api.resumeGate("desktop resume")
          : await api.pauseGate("desktop emergency stop");
      setGateStatus(status);
    } catch (err) {
      showToast("error", t("Failed to update execution gate: {error}", { error: String(err) }));
    } finally {
      setGateBusy(false);
    }
  }

  async function handleApprove(approval: ApprovalRequest) {
    try {
      await api.approvalApprove(approval.id);
      setPendingApprovals((prev) => prev.filter((a) => a.id !== approval.id));
    } catch (err) {
      showToast("error", t("Failed to approve: {error}", { error: String(err) }));
    }
  }

  async function handleReject(approval: ApprovalRequest) {
    try {
      await api.approvalReject(approval.id);
      setPendingApprovals((prev) => prev.filter((a) => a.id !== approval.id));
    } catch (err) {
      showToast("error", t("Failed to reject: {error}", { error: String(err) }));
    }
  }

  async function handleRemoveHost(name: string) {
    try {
      await api.removeHost(name);
      if (selectedHost === name) setSelectedHost("");
      await refresh();
    } catch (err) {
      showToast("error", String(err));
    }
  }

  // V3-3: batch remove from the Host table's row-selection action bar.
  async function handleBatchRemoveHosts(names: string[]) {
    try {
      await Promise.all(names.map((name) => api.removeHost(name)));
      if (names.includes(selectedHost)) setSelectedHost("");
      await refresh();
    } catch (err) {
      showToast("error", String(err));
    }
  }

  async function handleImportConfig() {
    try {
      await api.importSshConfig();
      await refresh();
    } catch (err) {
      showToast("error", String(err));
    }
  }

  function openModule(id: (typeof MODULES)[number]["id"]) {
    setActiveModule(id);
  }

  const activeModuleMeta = MODULES.find((module) => module.id === activeModule) ?? MODULES[0];
  const ActiveModuleIcon = activeModuleMeta.icon;
  const relatedModules = (RELATED_MODULES[activeModule] ?? [])
    .map((id) => MODULES.find((module) => module.id === id))
    .filter((module): module is (typeof MODULES)[number] => Boolean(module));

  function updateConnectionProgress(next: ConnectionProgress | null) {
    setConnectionProgress(next);
  }

  function showConnectionProgress(
    host: string,
    message: string,
    percent: number,
    title = t("Connecting to {host}", { host })
  ) {
    updateConnectionProgress({
      host,
      title,
      message,
      percent,
      busy: true,
      variant: "active",
    });
  }

  function completeConnectionProgress(host: string, message: string) {
    updateConnectionProgress({
      host,
      title: t("Connection complete"),
      message,
      percent: 100,
      busy: false,
      variant: "success",
    });
    window.setTimeout(() => {
      setConnectionProgress((current) =>
        current?.host === host && current.variant === "success" ? null : current
      );
    }, 900);
  }

  function failConnectionProgress(host: string, message: string) {
    updateConnectionProgress({
      host,
      title: t("Connection failed"),
      message,
      percent: 100,
      busy: false,
      variant: "error",
    });
  }

  async function handleConnect(name: string) {
    showConnectionProgress(name, t("Checking SSH fingerprint..."), 15);
    try {
      const status = await api.getHostFingerprintStatus(name);
      if (status.expectedFingerprintSha256 && !status.trusted) {
        updateConnectionProgress(null);
        setFingerprintPrompt({ host: name, status });
        return;
      }
      showConnectionProgress(name, t("Opening SSH connection..."), 60);
      await api.sshConnect(name);
      showConnectionProgress(name, t("Refreshing connection status..."), 90);
      await pollConnections();
      completeConnectionProgress(name, t("Connected to {host}", { host: name }));
    } catch (err) {
      const message = String(err);
      if (message.includes("SSH host fingerprint changed")) {
        try {
          showConnectionProgress(name, t("Verifying changed SSH fingerprint..."), 35);
          const status = await api.getHostFingerprintStatus(name);
          if (status.trusted) {
            showConnectionProgress(name, t("Opening SSH connection..."), 60);
            await api.sshConnect(name);
            showConnectionProgress(name, t("Refreshing connection status..."), 90);
            await pollConnections();
            completeConnectionProgress(name, t("Connected to {host}", { host: name }));
            return;
          }
          updateConnectionProgress(null);
          setFingerprintPrompt({ host: name, status });
          return;
        } catch (probeErr) {
          const errorMessage = t("Failed to verify host fingerprint: {error}", {
            error: String(probeErr),
          });
          failConnectionProgress(name, errorMessage);
          return;
        }
      }
      const errorMessage = t("Failed to connect: {error}", { error: message });
      failConnectionProgress(name, errorMessage);
    }
  }

  async function handleTrustFingerprintAndReconnect() {
    if (!fingerprintPrompt) return;
    setFingerprintBusy(true);
    const progressHost = fingerprintPrompt.host;
    showConnectionProgress(
      progressHost,
      t("Updating trusted SSH fingerprint..."),
      30,
      t("Updating SSH fingerprint")
    );
    try {
      const { host, status } = fingerprintPrompt;
      await api.trustHostFingerprint({
        host,
        expectedFingerprintSha256: status.expectedFingerprintSha256 ?? null,
        hostKeyAlgorithm: status.hostKeyAlgorithm,
        fingerprintSha256: status.fingerprintSha256,
      });
      showConnectionProgress(host, t("Reconnecting with updated fingerprint..."), 65);
      setFingerprintPrompt(null);
      await api.sshConnect(host);
      showConnectionProgress(host, t("Refreshing connection status..."), 90);
      await pollConnections();
      completeConnectionProgress(host, t("Connected to {host}", { host }));
    } catch (err) {
      const errorMessage = t("Failed to update host fingerprint: {error}", {
        error: String(err),
      });
      failConnectionProgress(progressHost, errorMessage);
    } finally {
      setFingerprintBusy(false);
    }
  }

  async function handleDisconnect(name: string) {
    try {
      await api.sshDisconnect(name);
      await pollConnections();
    } catch (err) {
      showToast("error", t("Failed to disconnect: {error}", { error: String(err) }));
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
          refresh().catch((err) => showToast("error", String(err)));
        }}
        onSkip={() => {
          setShowWizard(false);
          setWizardDismissed(true);
        }}
      />
    );
  }

  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      {/* K1: master-password unlock gate. Reloads hosts after unlock so resolved
          passwords are reflected. */}
      {secretsLocked && (
        <SecretsUnlock
          onUnlocked={() => {
            setSecretsLocked(false);
            void refresh().catch((err) => showToast("error", String(err)));
          }}
        />
      )}

      <NotificationCenter />

      <CommandPalette
        open={commandPaletteOpen}
        onClose={() => setCommandPaletteOpen(false)}
        modules={MODULES}
        hosts={hosts}
        onNavigateModule={(id) => openModule(id as (typeof MODULES)[number]["id"])}
        onSelectHost={setSelectedHost}
      />

      {/* Approval dialog overlay (Fix-1) */}
      {currentApproval && (
        <ApprovalDialog
          command={currentApproval.command}
          riskLevel={currentApproval.risk_level}
          onConfirm={() => handleApprove(currentApproval)}
          onCancel={() => handleReject(currentApproval)}
        />
      )}

      {fingerprintPrompt && (
        <Dialog
          onClose={fingerprintBusy ? undefined : () => setFingerprintPrompt(null)}
          className="max-w-lg"
        >
          <div className="flex items-start justify-between gap-4">
            <div className="min-w-0">
              <h3 className="text-lg font-bold">{t("SSH host fingerprint changed")}</h3>
              <p className="mt-1 text-sm text-muted-foreground">
                {t(
                  "The saved SSH fingerprint for this host does not match the server currently reached."
                )}
              </p>
            </div>
            <button
              type="button"
              className="rounded-md p-1 text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-50"
              onClick={() => setFingerprintPrompt(null)}
              disabled={fingerprintBusy}
              title={t("Cancel")}
            >
              <X size={18} />
            </button>
          </div>
          <div className="mt-5 grid gap-3 text-sm">
            <div>
              <div className="text-xs font-semibold uppercase text-muted-foreground">
                {t("Host")}
              </div>
              <div className="mt-1 font-mono text-xs text-foreground">
                {fingerprintPrompt.host} ({fingerprintPrompt.status.address})
              </div>
            </div>
            <div>
              <div className="text-xs font-semibold uppercase text-muted-foreground">
                {t("Saved fingerprint")}
              </div>
              <div className="mt-1 break-all rounded-md border border-border bg-muted px-3 py-2 font-mono text-xs">
                {fingerprintPrompt.status.expectedHostKeyAlgorithm ?? t("Unknown")}{" "}
                {fingerprintPrompt.status.expectedFingerprintSha256 ?? t("Not recorded")}
              </div>
            </div>
            <div>
              <div className="text-xs font-semibold uppercase text-muted-foreground">
                {t("Current fingerprint")}
              </div>
              <div className="mt-1 break-all rounded-md border border-warning/40 bg-warning/10 px-3 py-2 font-mono text-xs text-foreground">
                {fingerprintPrompt.status.hostKeyAlgorithm}{" "}
                {fingerprintPrompt.status.fingerprintSha256}
              </div>
            </div>
          </div>
          <div className="mt-5 flex justify-end gap-2">
            <Button
              variant="secondary"
              onClick={() => setFingerprintPrompt(null)}
              disabled={fingerprintBusy}
            >
              {t("Cancel")}
            </Button>
            <Button onClick={handleTrustFingerprintAndReconnect} disabled={fingerprintBusy}>
              {fingerprintBusy ? t("Updating...") : t("Trust and reconnect")}
            </Button>
          </div>
        </Dialog>
      )}

      {connectionProgress && (
        <Dialog
          onClose={
            connectionProgress.busy ? undefined : () => setConnectionProgress(null)
          }
          className="max-w-md"
        >
          <div className="flex items-start justify-between gap-4">
            <div className="min-w-0">
              <h3 className="text-lg font-bold">{connectionProgress.title}</h3>
              <p className="mt-1 break-words text-sm text-muted-foreground">
                {connectionProgress.message}
              </p>
            </div>
            <button
              type="button"
              className="rounded-md p-1 text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-50"
              onClick={() => setConnectionProgress(null)}
              disabled={connectionProgress.busy}
              title={t("Close")}
            >
              <X size={18} />
            </button>
          </div>

          <div className="mt-5 rounded-lg border border-border bg-muted/50 px-3 py-3">
            <div className="flex items-center gap-2 text-sm font-medium">
              {connectionProgress.busy && <Loader2 size={15} className="animate-spin" />}
              <span className="truncate">{connectionProgress.host}</span>
              <span className="ml-auto tabular-nums text-muted-foreground">
                {Math.round(connectionProgress.percent)}%
              </span>
            </div>
            <div className="mt-2 h-2 overflow-hidden rounded bg-border">
              <div
                className={cn(
                  "h-full rounded transition-all duration-300",
                  connectionProgress.variant === "error"
                    ? "bg-destructive"
                    : connectionProgress.variant === "success"
                      ? "bg-success"
                      : "bg-primary"
                )}
                style={{
                  width: `${Math.max(5, Math.min(100, connectionProgress.percent))}%`,
                }}
              />
            </div>
            <div className="mt-2 text-xs leading-snug text-muted-foreground">
              {connectionProgress.busy
                ? t("Please wait. The app is still responsive while SSH connects.")
                : connectionProgress.variant === "success"
                  ? t("Connection status updated.")
                  : t("You can close this dialog and retry from Host Management.")}
            </div>
          </div>

          {!connectionProgress.busy && (
            <div className="mt-5 flex justify-end">
              <Button variant="secondary" onClick={() => setConnectionProgress(null)}>
                {t("Close")}
              </Button>
            </div>
          )}
        </Dialog>
      )}

      <main className={cn("flex min-h-0 flex-1 overflow-hidden", !sidebarCollapsed && "max-lg:flex-col max-lg:overflow-y-auto")}>
      <aside
        className={cn(
          "flex shrink-0 flex-col gap-5 overflow-y-auto bg-sidebar p-5 text-sidebar-foreground",
          sidebarCollapsed
            ? "w-[76px] items-center p-3"
            : "lg:w-[280px] max-lg:w-full max-lg:overflow-visible"
        )}
      >
        <div className={cn("flex items-center gap-3", sidebarCollapsed && "flex-col gap-2")}>
          <div className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-sidebar-accent/15 text-sidebar-accent">
            <Terminal size={20} />
          </div>
          <div className={cn("min-w-0", sidebarCollapsed && "hidden")}>
            <h1 className="text-lg font-bold leading-tight">Agent2SSH</h1>
            <span className="block text-xs text-sidebar-foreground/55">
              {t("Local SSH capability layer")}
            </span>
          </div>
          <IconButton
            className={cn(
              "ml-auto shrink-0 border-white/15 bg-transparent text-sidebar-foreground/70 hover:bg-white/10 hover:text-sidebar-foreground",
              sidebarCollapsed && "ml-0"
            )}
            onClick={() => setSidebarCollapsed((value) => !value)}
            title={sidebarCollapsed ? t("Expand sidebar") : t("Collapse sidebar")}
          >
            {sidebarCollapsed ? <PanelLeftOpen size={15} /> : <PanelLeftClose size={15} />}
          </IconButton>
        </div>
        <nav
          className={cn("flex flex-col gap-1", !sidebarCollapsed && "max-lg:flex-row max-lg:flex-wrap")}
          aria-label={t("Modules")}
        >
          <div
            className={cn(
              "px-2 pb-1 text-[11px] font-bold uppercase tracking-wider text-sidebar-foreground/45",
              sidebarCollapsed ? "hidden" : "max-lg:hidden"
            )}
          >
            {t("Modules")}
          </div>
          {MODULES.map(({ id, label, icon: Icon }) => {
            const active = activeModule === id;
            const showBadge = id === "approvals" && pendingApprovals.length > 0;
            return (
              <button
                key={id}
                type="button"
                title={sidebarCollapsed ? t(label) : undefined}
                className={cn(
                  "relative flex items-center gap-2.5 rounded-lg px-3 py-2 text-left text-sm font-medium transition-colors",
                  sidebarCollapsed && "justify-center px-2",
                  active
                    ? "bg-sidebar-accent text-white shadow-sm"
                    : "text-sidebar-foreground/80 hover:bg-white/5 hover:text-sidebar-foreground"
                )}
                onClick={() => openModule(id)}
              >
                <Icon size={15} className={active ? "shrink-0" : "shrink-0 opacity-70"} />
                <span className={cn(sidebarCollapsed && "hidden")}>{t(label)}</span>
                {showBadge && (
                  <span
                    className={cn(
                      "size-2 rounded-full bg-warning",
                      sidebarCollapsed ? "absolute right-1.5 top-1.5" : "ml-auto"
                    )}
                  />
                )}
              </button>
            );
          })}
        </nav>
        <div className={cn("w-full", sidebarCollapsed && "hidden")}>
          <PingPanel hosts={hosts} />
        </div>
        {pendingApprovals.length > 0 && (
          <div
            className={cn(
              "flex items-center gap-2 rounded-lg border border-warning/30 bg-warning/10 px-3 py-2 text-sm font-medium text-warning",
              sidebarCollapsed && "hidden"
            )}
          >
            <span className="size-2 animate-pulse rounded-full bg-warning" />
            {pendingApprovals.length}{" "}
            {t(pendingApprovals.length > 1 ? "pending approvals" : "pending approval")}
          </div>
        )}
      </aside>

      <section
        className={cn(
          "flex min-h-0 min-w-0 flex-1 flex-col gap-5 overflow-y-auto p-6",
          !sidebarCollapsed && "max-lg:overflow-visible"
        )}
      >
        <header className="flex items-start justify-between gap-4 max-md:flex-col">
          <div className="min-w-0">
            <h2 className="flex items-center gap-2 text-xl font-bold">
              <ActiveModuleIcon size={20} className="text-muted-foreground" />
              {t(activeModuleMeta.label)}
            </h2>
            <p className="mt-1 text-sm text-muted-foreground">
              {activeModule === "hosts" && currentHost
                ? `${currentHost.user ? `${currentHost.user}@` : ""}${currentHost.host}:${currentHost.port ?? 22}${currentHost.jump_host ? ` via ${currentHost.jump_host}` : ""}${currentHostProxy ? ` · ${t("proxy")} ${currentHostProxy.name}` : ""}`
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

        {relatedModules.length > 0 && (
          <Breadcrumb
            current={activeModuleMeta}
            related={relatedModules}
            onNavigate={(id) => openModule(id as (typeof MODULES)[number]["id"])}
          />
        )}

        <section className="module-page">
          {activeModule === "dashboard" && (
            <Dashboard
              hosts={hosts}
              connectionStatuses={connectionStatuses}
              pendingApprovalsCount={pendingApprovals.length}
              secretsLocked={secretsLocked}
              daemonHealth={daemonHealth}
              onNavigateModule={(id) => openModule(id as (typeof MODULES)[number]["id"])}
            />
          )}

          {activeModule === "hosts" && (
            <div className="grid grid-cols-[minmax(0,1.4fr)_minmax(340px,0.6fr)] items-start gap-[18px] max-lg:grid-cols-1">
              <HostList
                hosts={hosts}
                groups={groups}
                proxies={proxies}
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
                onBatchRemove={handleBatchRemoveHosts}
                onRefresh={refresh}
                onConnect={handleConnect}
                onDisconnect={handleDisconnect}
              />
              <AddHostForm
                hosts={hosts}
                groups={groups}
                proxies={proxies}
                initialGroup={selectedGroup}
                editingHost={editingHost}
                onCancelEdit={() => setEditingHost(null)}
                onSaved={handleHostSaved}
              />
            </div>
          )}

          {activeModule === "proxies" && (
            <ProxyPanel proxies={proxies} hosts={hosts} onChanged={handleProxyChanged} />
          )}

          {activeModule === "terminal" && (
            <TerminalPanel hosts={hosts} initialHost={selectedHost} />
          )}

          {activeModule === "execute" && (
            <div className="grid gap-[18px]">
              <ExecPanel hosts={hosts} initialHost={selectedHost} onExecComplete={refreshAudit} />
              <MultiExecPanel hosts={hosts} onExecComplete={refreshAudit} />
            </div>
          )}

          {activeModule === "files-sessions" && (
            <SFTPPanel hosts={hosts} initialHost={selectedHost} />
          )}

          {activeModule === "tunnels" && (
            <ForwardPanel
              hosts={hosts}
              initialHost={selectedHost}
              onChanged={handleForwardChanged}
            />
          )}

          {activeModule === "activity" && <LiveActivityPanel />}

          {activeModule === "mcp-agents" && <McpAgentsPanel />}

          {activeModule === "sync" && <SyncPanel />}

          {activeModule === "keys" && <KeysPanel onChanged={handleKeyChanged} />}

          {activeModule === "playbooks" && <PlaybooksPanel hosts={hosts} />}

          {activeModule === "audit" && (
            <div className="grid gap-[18px]">
              <AuditCharts />
              <AuditPanel audit={audit} onRefresh={refreshAudit} />
            </div>
          )}

          {activeModule === "help" && <HelpPanel />}

          {activeModule === "approvals" && <ApprovalTimeline />}

          {activeModule === "config" && <ConfigSnapshotsPanel />}

          {activeModule === "topology" && (
            <ConnectionTopology
              hosts={hosts}
              proxies={proxies}
              connectionStatuses={connectionStatuses}
              daemonHealth={daemonHealth}
              onSelectHost={setSelectedHost}
              onNavigateModule={(id) => openModule(id as (typeof MODULES)[number]["id"])}
            />
          )}
        </section>
      </section>
      </main>

      <Footbar
        daemonHealth={daemonHealth}
        gateStatus={gateStatus}
        secretsLocked={secretsLocked}
        connectionStatuses={connectionStatuses}
      />
    </div>
  );
}
