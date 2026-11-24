import {
  KeyRound,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  RefreshCw,
  X,
} from "lucide-react";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, reportError } from "./api";
import { isAlreadyDecided } from "./lib/approval-decision";
import { MODULES, RELATED_MODULES, FULL_HEIGHT_MODULES, SHORTCUT_MODULES, SIDEBAR_GROUPS, type ModuleId } from "./nav";
import { moduleSubtitle } from "./moduleSubtitle";
import { DockedView, TerminalDock } from "./features/terminal/dock";
import LiveTerminal from "./features/terminal/LiveTerminal";
import AddHostForm from "./components/AddHostForm";
import ApprovalWorkbench from "./components/approval/ApprovalWorkbench";
import AuditWorkbench from "./components/AuditWorkbench";
import RelatedActionsBar from "./components/RelatedActionsBar";
import ConfigSnapshotsPanel from "./components/ConfigSnapshotsPanel";
import ContainersPanel from "./components/ContainersPanel";
import ResidentSessionsPanel from "./components/ResidentSessionsPanel";
import ConnectionTopology from "./components/ConnectionTopology";
import CommandPalette, {
  type CommandPaletteModule,
  type PaletteQuickAction,
} from "./components/CommandPalette";
import Dashboard from "./components/Dashboard";
import LocalEnvironmentsCard from "./components/LocalEnvironmentsCard";
import ExecutionWorkbench from "./components/ExecutionWorkbench";
import ForwardPanel from "./components/ForwardPanel";
import HelpPanel from "./components/HelpPanel";
import HostDrawer from "./components/HostDrawer";
import HostList, { ALL_GROUPS_ID, type HostConnectionProgress } from "./components/HostList";
import NetcattyImportDialog from "./components/NetcattyImportDialog";
import KeysPanel from "./components/KeysPanel";
import LiveActivityPanel from "./components/LiveActivityPanel";
import McpAgentsPanel from "./components/McpAgentsPanel";
import NotificationCenter from "./components/NotificationCenter";
import ProxyPanel from "./components/ProxyPanel";
import RecordingsPanel from "./components/RecordingsPanel";
import SFTPPanel from "./components/SFTPPanel";
import TerminalDrawer from "./components/TerminalDrawer";
import TerminalPanel from "./components/TerminalPanel";
import SecretsUnlock from "./components/SecretsUnlock";
import SettingsMenu from "./components/SettingsMenu";
import SetupWizard from "./components/SetupWizard";
import SyncPanel from "./components/SyncPanel";
import TopBar from "./components/TopBar";
import { Button } from "./components/ui/button";
import { ConfirmHost, Dialog } from "./components/ui/dialog";
import { IconButton } from "./components/ui/icon-button";
import { Spinner } from "./components/ui/state";
import { useToast } from "./components/ui/toast";
import { useAgentEvents } from "./eventsBus";
import { useI18n } from "./i18n";
import { cn } from "./lib/utils";
import type { ApprovalRequest, AuditEntry, ConnectionStatus, DaemonHealth, ExecutionGateStatus, HostFingerprintStatus, HostGroup, HostProfile, ProxyProfile } from "./types";

const APPROVAL_POLL_MS = 2000;
const DAEMON_START_RECHECK_MS = 700;
const WEBDAV_AUTO_SYNC_MS = 10 * 60 * 1000;

// Explicit refresh (SettingsMenu button) always bumps the "checked at" clock;
// background polls only bump it — and re-render the tree — when the polled
// value actually changed. Without this the 5s/10s polls re-render the whole
// app twice per 5 seconds even when nothing changed.
type PollOptions = { bumpCheckedAt?: boolean };

// Poll setState guard: keep the previous state object when a fresh poll
// result is structurally identical, so the 2s/5s/10s polls only re-render
// the app (and re-trigger downstream memos/effects, e.g. the topology force
// simulation) when something actually changed.
function keepIfEqual<T>(prev: T, next: T): T {
  return JSON.stringify(prev) === JSON.stringify(next) ? prev : next;
}

// One console-mode sidebar entry. Memoized: a module switch flips `active` on
// exactly two items, so per-item memo keeps the rail from rebuilding on every
// click.
const SidebarItem = memo(function SidebarItem({
  id,
  active,
  collapsed,
  pendingCount,
  onOpen,
}: {
  id: ModuleId;
  active: boolean;
  collapsed: boolean;
  pendingCount: number;
  onOpen: (id: ModuleId) => void;
}) {
  const { t } = useI18n();
  const meta = MODULES.find((module) => module.id === id) ?? MODULES[0];
  const { label, icon: Icon } = meta;
  const badge = id === "approvals" && pendingCount > 0;
  return (
    <button
      type="button"
      title={collapsed ? t(label) : undefined}
      aria-current={active ? "page" : undefined}
      className={cn(
        "relative flex items-center gap-2 rounded-lg px-2.5 py-1 text-left text-[13px] font-medium transition-colors",
        collapsed && "justify-center px-2",
        active
          ? "bg-sidebar-accent text-primary-foreground"
          : "text-sidebar-foreground/80 hover:bg-white/5 hover:text-sidebar-foreground"
      )}
      onClick={() => onOpen(id)}
    >
      <Icon size={14} className={active ? "shrink-0" : "shrink-0 opacity-70"} />
      <span className={cn("truncate", collapsed && "hidden")}>{t(label)}</span>
      {badge && !collapsed && (
        <span
          className={cn(
            "ml-auto shrink-0 rounded-full px-1.5 font-mono text-[10px] font-bold leading-4",
            active ? "bg-canvas-0 text-primary" : "border border-line text-muted-foreground"
          )}
        >
          {pendingCount}
        </span>
      )}
      {badge && collapsed && (
        <span className="absolute right-1.5 top-1.5 size-2 rounded-full bg-risk-medium" />
      )}
    </button>
  );
});


export default function App() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [hosts, setHosts] = useState<HostProfile[]>([]);
  const [groups, setGroups] = useState<HostGroup[]>([{ id: "default", name: "Default", color: "#4A6CF7", sort_order: 0 }]);
  const [proxies, setProxies] = useState<ProxyProfile[]>([]);
  const [selectedHost, setSelectedHost] = useState("");
  const [execInitialCommand, setExecInitialCommand] = useState<string | undefined>();
  // Bumps on every approval jump so the same command replaces an unexecuted draft.
  const [execCarryEpoch, setExecCarryEpoch] = useState(0);
  // One-shot. ExecutionWorkbench applies it and clears it, so the next visit
  // restores `agent2ssh.execMode` instead of sticking on single.
  const [execPreferredMode, setExecPreferredMode] = useState<
    "single" | "multi" | "playbooks" | undefined
  >(undefined);
  const [selectedGroup, setSelectedGroup] = useState(ALL_GROUPS_ID);
  const [editingHost, setEditingHost] = useState<HostProfile | null>(null);
  const [isHostDrawerOpen, setIsHostDrawerOpen] = useState(false);
  const [highlightedHost, setHighlightedHost] = useState<string | null>(null);
  const [netcattyOpen, setNetcattyOpen] = useState(false);
  const [activeModule, setActiveModule] = useState<(typeof MODULES)[number]["id"]>("dashboard");
  const [loading, setLoading] = useState(true);
  const [pendingApprovals, setPendingApprovals] = useState<ApprovalRequest[]>([]);
  const [connectionStatuses, setConnectionStatuses] = useState<ConnectionStatus[]>([]);
  // K1: true when the app-managed credential store is initialized but locked,
  // gating the UI behind a master-password unlock dialog at startup.
  const [secretsLocked, setSecretsLocked] = useState(false);
  // v3 T05: set by the ⌘K "unlock credentials" quick action to re-surface the
  // unlock gate even when the store was already unlocked at startup.
  const [unlockRequested, setUnlockRequested] = useState(false);
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
  const [connectionProgress, setConnectionProgress] = useState<HostConnectionProgress | null>(null);
  // v3 T05: right-side terminal drawer. Raised by the approval workbench
  // ("approve and execute" — command=null, the daemon executes the approved
  // command itself) and by audit rows ("replay" — the historical command is
  // typed into a fresh PTY once the session connects).
  const [terminalDrawer, setTerminalDrawer] = useState<{
    id: string;
    host: string;
    command: string | null;
  } | null>(null);
  // PTYs opened from the drawer. `sessionId` is null while the view sits in
  // the drawer, then the id of the main-terminal tab that adopted it.
  const [dockedPtys, setDockedPtys] = useState<
    Array<{ id: string; host: string; command: string | null; sessionId: string | null }>
  >([]);
  const [drawerConnected, setDrawerConnected] = useState(false);
  const dockSeq = useRef(0);
  // Bumped on each "Open in Terminal" so a repeat click to the same host
  // still focuses its tab. `dockId` is the PTY that moves with the view.
  const [terminalFocusRequest, setTerminalFocusRequest] = useState<{
    host: string;
    dockId: string | null;
    nonce: number;
  } | null>(null);
  const [approvalFocusRequest, setApprovalFocusRequest] = useState<{
    id: string;
    nonce: number;
  } | null>(null);
  // F5: the anomaly card opens Activity on the attention filter. The epoch
  // re-applies that filter when the card is opened again after a switch to all.
  const [activityPreset, setActivityPreset] = useState<"all" | "attention">("all");
  const [activityPresetEpoch, setActivityPresetEpoch] = useState(0);
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
  // Previous poll signatures: background polls bump gateCheckedAt /
  // daemonHealthCheckedAt (and thus re-render) only on a real change.
  const gateSignatureRef = useRef<string | null>(null);
  const daemonSignatureRef = useRef<string | null>(null);
  const webDavSyncInFlight = useRef(false);
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

  const openModule = useCallback((id: (typeof MODULES)[number]["id"]) => {
    if (id === "playbooks") {
      setExecPreferredMode("playbooks");
      setActiveModule("execute");
      return;
    }
    setActiveModule(id);
  }, []);

  // v3 §1.3: jump to the approval workbench and focus the requested pending
  // card (or the first card when opened from ⌘⇧A / TopBar).
  const openApprovalsWorkbench = useCallback(
    (approvalId?: string) => {
      const targetId = typeof approvalId === "string" && approvalId ? approvalId : undefined;
      if (targetId) {
        setApprovalFocusRequest((current) => ({
          id: targetId,
          nonce: (current?.nonce ?? 0) + 1,
        }));
      }
      openModule("approvals");
      if (!targetId) {
        window.setTimeout(() => {
          document.getElementById("approval-queue-card-0")?.focus();
        }, 80);
      }
    },
    [openModule],
  );

  const refresh = useCallback(async () => {
    const [hostList, groupList, proxyList] = await Promise.all([
      api.listHosts(),
      api.listHostGroups(),
      api.listProxies(),
    ]);
    setHosts(hostList);
    setGroups(groupList.length > 0 ? groupList : [{ id: "default", name: "Default", color: "#4A6CF7", sort_order: 0 }]);
    setProxies(proxyList);
    setSelectedHost((current) => {
      if (current && hostList.some((host) => host.name === current)) {
        return current;
      }
      return hostList[0]?.name ?? "";
    });
  }, []);

  // AuditWorkbench owns its own fetch and SSE refresh, so execution completion
  // does not need to fire a discarded audit read here.
  const handleExecComplete = useCallback(() => {}, []);

  const runAutoWebDavSync = useCallback(async (reason: string) => {
    if (webDavSyncInFlight.current) return;
    webDavSyncInFlight.current = true;
    try {
      const config = await api.getWebDavSyncConfig();
      if (!config.enabled || !config.url.trim() || !config.remotePath.trim()) {
        return;
      }
      const status = await api.pushWebDavSync();
      // The command resolves even when the upload fails, so failure has to be
      // read off the status — `catch` never fires for a rejected sync.
      if (status.lastSuccess === false) {
        reportError(
          "webdav-auto-sync",
          "automatic webdav sync failed",
          new Error(status.lastMessage ?? "unknown"),
          { reason }
        );
      }
    } catch (err) {
      reportError("webdav-auto-sync", "automatic webdav sync failed", err, { reason });
    } finally {
      webDavSyncInFlight.current = false;
    }
  }, []);

  const handleHostSaved = useCallback(async (name: string) => {
    await refresh();
    void runAutoWebDavSync("host_saved");
    setIsHostDrawerOpen(false);
    setEditingHost(null);
    setSelectedHost(name);
    setHighlightedHost(name);
  }, [refresh, runAutoWebDavSync]);

  useEffect(() => {
    if (!highlightedHost) return;
    const timer = window.setTimeout(() => setHighlightedHost(null), 2600);
    return () => window.clearTimeout(timer);
  }, [highlightedHost]);

  const handleProxyChanged = useCallback(async () => {
    await refresh();
    void runAutoWebDavSync("proxy_changed");
  }, [refresh, runAutoWebDavSync]);

  const handleForwardChanged = useCallback(() => {
    void runAutoWebDavSync("forward_changed");
  }, [runAutoWebDavSync]);

  const handleKeyChanged = useCallback(() => {
    void runAutoWebDavSync("key_changed");
  }, [runAutoWebDavSync]);

  function groupIdFromName(name: string): string {
    const base = name
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9._-]+/g, "-")
      .replace(/^-+|-+$/g, "");
    return base || `group-${Date.now()}`;
  }

  const handleCreateGroup = useCallback(
    async (name: string) => {
      const trimmed = name.trim();
      if (!trimmed) return;
      try {
        let id = groupIdFromName(trimmed);
        let suffix = 2;
        while (groups.some((group) => group.id === id)) {
          id = `${groupIdFromName(trimmed)}-${suffix}`;
          suffix += 1;
        }
        await api.saveHostGroup({ id, name: trimmed, color: "#4A6CF7", sort_order: 0 });
        setSelectedGroup(id);
        await refresh();
      } catch (err) {
        showToast("error", String(err));
      }
    },
    [groups, refresh, showToast],
  );

  const handleRenameGroup = useCallback(
    async (id: string, name: string) => {
      const trimmed = name.trim();
      if (!trimmed) return;
      try {
        await api.saveHostGroup({ id, name: trimmed, color: "#4A6CF7", sort_order: 0 });
        await refresh();
      } catch (err) {
        showToast("error", String(err));
      }
    },
    [refresh, showToast],
  );

  const handleDeleteGroup = useCallback(
    async (id: string) => {
      if (id === "default") return;
      try {
        await api.deleteHostGroup(id);
        if (selectedGroup === id) setSelectedGroup("default");
        await refresh();
      } catch (err) {
        showToast("error", String(err));
      }
    },
    [selectedGroup, refresh, showToast],
  );

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

  const pollGateStatus = useCallback(async (options?: PollOptions) => {
    const status = await api.getGateStatus();
    setGateStatus((prev) => keepIfEqual(prev, status));
    const signature = JSON.stringify(status);
    const changed = signature !== gateSignatureRef.current;
    gateSignatureRef.current = signature;
    if (options?.bumpCheckedAt || changed) {
      setGateCheckedAt(Date.now());
    }
  }, []);

  useEffect(() => {
    const id = setInterval(pollGateStatus, 5000);
    pollGateStatus();
    return () => clearInterval(id);
  }, [pollGateStatus]);

  const pollDaemonHealth = useCallback(async (options?: PollOptions) => {
    const health = await api.getDaemonHealth();
    setDaemonHealth((prev) => keepIfEqual(prev, health));
    const signature = JSON.stringify(health);
    const changed = signature !== daemonSignatureRef.current;
    daemonSignatureRef.current = signature;
    if (options?.bumpCheckedAt || changed) {
      setDaemonHealthCheckedAt(Date.now());
    }
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
  // Ctrl/Cmd+1..9 jumps to SHORTCUT_MODULES (the daily set, not the rail
  // order), Ctrl/Cmd+Shift+A surfaces the pending approval workbench.
  // Hosts only: Ctrl/Cmd+N opens the add-host drawer and does not become a
  // browser shortcut. Other modules leave Cmd/Ctrl+N alone.
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
        // v3: the auto approval dialog is gone — jump to the workbench and
        // focus the first pending card instead (plan §1.3 / §5 risk #3).
        openApprovalsWorkbench();
        return;
      }

      if (!e.shiftKey && key === "n" && activeModule === "hosts") {
        e.preventDefault();
        setEditingHost(null);
        setIsHostDrawerOpen(true);
        return;
      }

      if (!e.shiftKey && key >= "1" && key <= "9") {
        const index = Number(key) - 1;
        const moduleId = SHORTCUT_MODULES[index];
        if (moduleId) {
          e.preventDefault();
          openModule(moduleId);
        }
      }
    }
    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => window.removeEventListener("keydown", handleGlobalKeyDown);
  }, [showToast, t, openApprovalsWorkbench, openModule, activeModule]);

  const handleGateToggle = useCallback(async () => {
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
  }, [gateStatus, showToast, t]);

  const handleApprove = useCallback(
    async (approval: ApprovalRequest, command?: string) => {
      try {
        await api.approvalApprove(approval.id, command);
        setPendingApprovals((prev) => prev.filter((a) => a.id !== approval.id));
        // v3 §3.4: raise the terminal drawer as a live viewport onto the host.
        // The daemon runs the approved command itself (the original exec caller
        // was waiting on approval_wait), so the drawer must NOT re-type the
        // command — that would execute it twice.
        raiseTerminalDrawer(approval.host, null);
      } catch (err) {
        await reportUndecided(err, approval, "approve");
      }
    },
    // reportUndecided is a stable useCallback below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [showToast, t],
  );

  const handleReject = useCallback(
    async (approval: ApprovalRequest) => {
      try {
        await api.approvalReject(approval.id);
        setPendingApprovals((prev) => prev.filter((a) => a.id !== approval.id));
      } catch (err) {
        await reportUndecided(err, approval, "reject");
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [showToast, t],
  );

  /**
   * The daemon answers 400 for every refusal, so "already Approved" (the
   * request was decided and only the response was lost) is indistinguishable
   * from "the command is blocked" or "unknown approval" by status alone. The
   * message is the only thing that tells them apart, and only the first one may
   * retire the card here: a still-pending request must stay in the queue, or a
   * refusal would quietly drop the operator's decision.
   */
  const reportUndecided = useCallback(
    async (err: unknown, approval: ApprovalRequest, action: "approve" | "reject") => {
      showToast(
        "error",
        t(action === "approve" ? "Failed to approve: {error}" : "Failed to reject: {error}", {
          error: String(err),
        }),
      );
      if (!isAlreadyDecided(String(err))) return;
      // The review's L2-121: without this the card outlives every later click,
      // each one repeating the same 400, because the `approval_responded` event
      // that would have refreshed the list is the one that went missing. This
      // read only says what the daemon has on record; when it is unreachable the
      // entry `fetchApprovals` would substitute (`[]`) is no evidence of a
      // decision, so only drop an id the daemon itself listed as decided.
      const listed = (await api.fetchApprovals()).find((a) => a.id === approval.id);
      if (listed && listed.status !== "pending") {
        setPendingApprovals((prev) => prev.filter((a) => a.id !== approval.id));
      }
    },
    [showToast, t],
  );

  const handleRemoveHost = useCallback(
    async (name: string) => {
      try {
        await api.removeHost(name);
        if (selectedHost === name) setSelectedHost("");
        await refresh();
      } catch (err) {
        showToast("error", String(err));
      }
    },
    [selectedHost, refresh, showToast],
  );

  // V3-3: batch remove from the Host table's row-selection action bar.
  const handleBatchRemoveHosts = useCallback(
    async (names: string[]) => {
      try {
        await Promise.all(names.map((name) => api.removeHost(name)));
        if (names.includes(selectedHost)) setSelectedHost("");
        await refresh();
      } catch (err) {
        showToast("error", String(err));
      }
    },
    [selectedHost, refresh, showToast],
  );

  const handleImportConfig = useCallback(async () => {
    try {
      await api.importSshConfig();
      await refresh();
    } catch (err) {
      showToast("error", String(err));
    }
  }, [refresh, showToast]);

  // G13: the two halves of OpenSSH known_hosts interop. Import pulls the system
  // file's trust into the app-managed store; forget drops a host from the
  // system file (`ssh-keygen -R`) without touching the app's own store.
  const handleImportOpensshTrust = useCallback(async () => {
    try {
      const summary = await api.importKnownHosts();
      await refresh();
      showToast(
        "success",
        t("Imported {imported} trusted host(s) from OpenSSH, skipped {skipped}.", {
          imported: summary.imported,
          skipped: summary.skipped,
        }),
      );
    } catch (err) {
      showToast("error", String(err));
    }
  }, [refresh, showToast, t]);

  const handleForgetOpenssh = useCallback(
    async (host: HostProfile) => {
      try {
        const removed = await api.forgetSystemKnownHost(host.host, host.port ?? 22);
        showToast(
          removed > 0 ? "success" : "warning",
          removed > 0
            ? t("Removed {count} OpenSSH known_hosts entry(ies) for {name}.", {
                count: removed,
                name: host.host,
              })
            : t("{name} had no entry in the system OpenSSH known_hosts.", { name: host.host }),
        );
      } catch (err) {
        showToast("error", String(err));
      }
    },
    [showToast, t],
  );

  const openAddHostDrawer = useCallback(() => {
    setEditingHost(null);
    setIsHostDrawerOpen(true);
  }, []);

  // v3 §3.5: ⌘K quick actions — sync / unlock credentials / new host. The
  // unlock entry surfaces the credential gate (or the Keys module when already
  // unlocked); actions are rebuilt when language or lock state changes.
  const quickActions = useMemo<PaletteQuickAction[]>(
    () => [
      {
        id: "sync",
        label: t("Sync now"),
        icon: RefreshCw,
        run: () => {
          void api
            .pushWebDavSync()
            .then((status) => {
              if (status.lastSuccess === false) {
                showToast(
                  "error",
                  status.lastMessage ?? t("WebDAV sync upload failed.")
                );
                return;
              }
              showToast(
                "success",
                status.lastMessage ?? t("WebDAV sync upload completed.")
              );
            })
            .catch((err) => showToast("error", String(err)));
        },
      },
      {
        id: "unlock-secrets",
        label: t("Unlock credentials"),
        icon: KeyRound,
        run: () => {
          if (secretsLocked) setUnlockRequested(true);
          else openModule("keys");
        },
      },
      {
        id: "new-host",
        label: t("New host"),
        icon: Plus,
        run: () => {
          openModule("hosts");
          openAddHostDrawer();
        },
      },
    ],
    // openModule is a stable setState wrapper; showToast/t come from context.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [t, secretsLocked, openAddHostDrawer]
  );

  // Stable replacements for the inline closures previously passed as panel
  // props: with React.memo on the panels below, an unstable prop would defeat
  // the bail-out and re-render the active panel on every App state change.
  const openCommandPalette = useCallback(() => setCommandPaletteOpen(true), []);
  const closeCommandPalette = useCallback(() => setCommandPaletteOpen(false), []);
  const openNetcattyImport = useCallback(() => setNetcattyOpen(true), []);
  const closeNetcattyImport = useCallback(() => setNetcattyOpen(false), []);
  const closeHostDrawer = useCallback(() => {
    setIsHostDrawerOpen(false);
    setEditingHost(null);
  }, []);
  const openEditHostDrawer = useCallback((host: HostProfile) => {
    setEditingHost(host);
    setIsHostDrawerOpen(true);
  }, []);
  const raiseTerminalDrawer = useCallback((host: string, command: string | null) => {
    dockSeq.current += 1;
    const id = `dock-${dockSeq.current}`;
    setDrawerConnected(false);
    // A drawer that was never moved into the main terminal is replaced.
    setDockedPtys((prev) => [
      ...prev.filter((item) => item.sessionId),
      { id, host, command, sessionId: null },
    ]);
    setTerminalDrawer({ id, host, command });
  }, []);
  const closeTerminalDrawer = useCallback(() => {
    setTerminalDrawer((current) => {
      if (current) {
        setDockedPtys((prev) => prev.filter((item) => item.id !== current.id || item.sessionId));
      }
      return null;
    });
    setDrawerConnected(false);
  }, []);
  // The drawer's LiveTerminal stays mounted. Closing the chrome and focusing
  // the host tab makes TerminalPanel render a slot; DockedView portals the
  // same view into it, so the WebSocket is not opened again. The command is
  // not forwarded — null already ran on the daemon, and a replay already
  // typed into this view.
  const openDrawerInTerminal = useCallback(
    (host: string) => {
      const dockId = terminalDrawer?.id ?? null;
      setTerminalDrawer(null);
      setSelectedHost(host);
      setTerminalFocusRequest((current) => ({
        host,
        dockId,
        nonce: (current?.nonce ?? 0) + 1,
      }));
      openModule("terminal");
    },
    [openModule, terminalDrawer],
  );
  const adoptDock = useCallback((dockId: string, sessionId: string) => {
    setDockedPtys((prev) =>
      prev.map((item) => (item.id === dockId ? { ...item, sessionId } : item)),
    );
  }, []);
  const releaseDock = useCallback((sessionId: string) => {
    setDockedPtys((prev) => prev.filter((item) => item.sessionId !== sessionId));
  }, []);
  const openSetupWizard = useCallback(() => {
    setWizardDismissed(false);
    setShowWizard(true);
  }, []);
  const navigateModule = useCallback(
    (id: string) => openModule(id as (typeof MODULES)[number]["id"]),
    [openModule],
  );
  // Host chips on the proxy, key, and tunnel pages land on that host.
  const openHostInFleet = useCallback((name: string) => {
    setSelectedGroup(ALL_GROUPS_ID);
    setSelectedHost(name);
    setHighlightedHost(name);
    openModule("hosts");
  }, [openModule]);
  const requestCredentialUnlock = useCallback(() => {
    if (secretsLocked) setUnlockRequested(true);
  }, [secretsLocked]);
  const startDaemonFromDashboard = useCallback(async () => {
    try {
      await ensureLocalDaemon();
    } catch (err) {
      showToast("error", String(err));
    }
  }, [ensureLocalDaemon, showToast]);
  const openAnomalies = useCallback(() => {
    setActivityPreset("attention");
    setActivityPresetEpoch((epoch) => epoch + 1);
    openModule("activity");
  }, [openModule]);
  const replayAuditEntry = useCallback(
    (entry: AuditEntry) => raiseTerminalDrawer(entry.host, entry.command),
    [raiseTerminalDrawer],
  );
  const clearExecPreferredMode = useCallback(() => {
    setExecPreferredMode(undefined);
  }, []);
  const handleExecModeChange = useCallback((mode: "single" | "multi" | "playbooks") => {
    if (mode === "single" || mode === "multi") {
      setActiveModule((current) => (current === "playbooks" ? "execute" : current));
    }
  }, []);
  const goToExecuteForApproval = useCallback(
    (approval: ApprovalRequest, command?: string) => {
      setSelectedHost(approval.host);
      setExecInitialCommand(command ?? approval.command);
      setExecCarryEpoch((epoch) => epoch + 1);
      setExecPreferredMode("single");
      openModule("execute");
    },
    [openModule],
  );
  const goToExecuteFromAudit = useCallback(
    (entry: AuditEntry) => {
      setSelectedHost(entry.host);
      setExecInitialCommand(entry.command);
      setExecCarryEpoch((epoch) => epoch + 1);
      setExecPreferredMode("single");
      openModule("execute");
    },
    [openModule],
  );
  const openHostInTerminal = useCallback((host: string) => {
    setSelectedHost(host);
    setTerminalFocusRequest((current) => ({
      host,
      dockId: null,
      nonce: (current?.nonce ?? 0) + 1,
    }));
    openModule("terminal");
  }, [openModule]);
  const openHostInExecute = useCallback((host: string) => {
    setSelectedHost(host);
    setExecPreferredMode("single");
    openModule("execute");
  }, [openModule]);
  const openHostInFiles = useCallback((host: string) => {
    setSelectedHost(host);
    openModule("files-sessions");
  }, [openModule]);

  // TopBar's `actions` slot: memoized element identity keeps memo(TopBar) and
  // the memoized SettingsMenu from re-rendering on unrelated App state churn.
  const settingsActions = useMemo(
    () => (
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
        onOpenSetup={openSetupWizard}
      />
    ),
    [
      gateStatus,
      gateBusy,
      gateCheckedAt,
      daemonHealth,
      daemonHealthCheckedAt,
      handleGateToggle,
      pollGateStatus,
      pollDaemonHealth,
      handleImportConfig,
      openSetupWizard,
    ],
  );

  const activeModuleMeta = MODULES.find((module) => module.id === activeModule) ?? MODULES[0];
  const ActiveModuleIcon = activeModuleMeta.icon;
  const relatedModules = (RELATED_MODULES[activeModule] ?? [])
    .map((id) => MODULES.find((module) => module.id === id))
    .filter((module): module is (typeof MODULES)[number] => Boolean(module));

  const updateConnectionProgress = useCallback((next: HostConnectionProgress | null) => {
    setConnectionProgress(next);
  }, []);

  const showConnectionProgress = useCallback(
    (host: string, message: string, percent: number) => {
      updateConnectionProgress({
        host,
        message,
        percent,
        busy: true,
      });
    },
    [updateConnectionProgress],
  );

  const completeConnectionProgress = useCallback(
    (host: string, message: string) => {
      updateConnectionProgress({
        host,
        message,
        percent: 100,
        busy: false,
      });
      window.setTimeout(() => {
        setConnectionProgress((current) =>
          current?.host === host && !current.busy && current.message === message ? null : current
        );
      }, 900);
    },
    [updateConnectionProgress],
  );

  const failConnectionProgress = useCallback(
    (host: string, message: string) => {
      showToast("error", message);
      setConnectionProgress((current) => (current?.host === host ? null : current));
    },
    [showToast],
  );

  const handleConnect = useCallback(
    async (name: string) => {
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
    },
    [t, showConnectionProgress, completeConnectionProgress, failConnectionProgress, pollConnections, updateConnectionProgress],
  );

  const handleTrustFingerprintAndReconnect = useCallback(async () => {
    if (!fingerprintPrompt) return;
    setFingerprintBusy(true);
    const progressHost = fingerprintPrompt.host;
    showConnectionProgress(progressHost, t("Updating trusted SSH fingerprint..."), 30);
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
  }, [fingerprintPrompt, t, showConnectionProgress, completeConnectionProgress, failConnectionProgress, pollConnections]);

  const handleDisconnect = useCallback(
    async (name: string) => {
      try {
        await api.sshDisconnect(name);
        await pollConnections();
      } catch (err) {
        showToast("error", t("Failed to disconnect: {error}", { error: String(err) }));
      }
    },
    [pollConnections, showToast, t],
  );

  if (loading) {
    return (
      <main className="flex min-h-screen flex-col items-center justify-center gap-4 bg-background text-muted-foreground">
        <Spinner size={32} />
        <span>{t("Loading Agent2SSH...")}</span>
      </main>
    );
  }

  // Render setup wizard overlay when active
  if (showWizard) {
    return (
      <>
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
          onImportNetcatty={openNetcattyImport}
        />
        <NetcattyImportDialog
          open={netcattyOpen}
          onClose={closeNetcattyImport}
          onImported={refresh}
        />
      </>
    );
  }

  return (
    <TerminalDock>
    <div className="flex h-screen flex-col bg-background text-foreground">
      {/* K1: master-password unlock gate. Reloads hosts after unlock so resolved
          passwords are reflected. Also surfaced via the ⌘K unlock quick action. */}
      {(secretsLocked || unlockRequested) && (
        <SecretsUnlock
          onUnlocked={() => {
            setSecretsLocked(false);
            setUnlockRequested(false);
            void refresh().catch((err) => showToast("error", String(err)));
          }}
        />
      )}

      <NotificationCenter />

      <CommandPalette
        open={commandPaletteOpen}
        onClose={closeCommandPalette}
        modules={MODULES}
        hosts={hosts}
        pending={pendingApprovals}
        quickActions={quickActions}
        onNavigateModule={navigateModule}
        onSelectHost={openHostInFleet}
        onConnectHost={openHostInTerminal}
        onOpenApprovals={openApprovalsWorkbench}
      />

      {/* v3: approvals live in the always-on workbench; no auto dialog (Fix-1 retired). */}
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

      {/* v3: status top bar with agent pills + ⌘K entry. The actions element is
          memoized so memo(TopBar) can bail out when only unrelated App state
          (active module, dialogs, drawer) changed. */}
      <TopBar
        daemonHealth={daemonHealth}
        gateStatus={gateStatus}
        gateBusy={gateBusy}
        secretsLocked={secretsLocked}
        connectionStatuses={connectionStatuses}
        pendingApprovalsCount={pendingApprovals.length}
        onOpenPalette={openCommandPalette}
        onOpenApprovals={openApprovalsWorkbench}
        onStartDaemon={startDaemonFromDashboard}
        onGateToggle={handleGateToggle}
        onRequestUnlock={requestCredentialUnlock}
        onNavigateModule={navigateModule}
        connectionBusy={connectionProgress?.busy === true}
        actions={settingsActions}
      />

      <main className={cn("flex min-h-0 flex-1 overflow-hidden", !sidebarCollapsed && "max-lg:flex-col max-lg:overflow-y-auto")}>
      <aside
        className={cn(
          "flex shrink-0 flex-col gap-3 overflow-y-auto border-r border-line bg-sidebar p-3 text-sidebar-foreground",
          sidebarCollapsed
            ? "w-[68px] items-center"
            : "w-[200px] max-lg:w-full max-lg:overflow-visible"
        )}
      >
        <div className={cn("flex items-center gap-2", sidebarCollapsed && "flex-col")}>
          <div className="flex size-7 shrink-0 items-center justify-center rounded-md border border-edge font-mono text-[11px] font-bold text-foreground">
            A2
          </div>
          <span className={cn("min-w-0 truncate text-sm font-bold", sidebarCollapsed && "hidden")}>
            Agent2SSH
          </span>
          <IconButton
            className={cn(
              "ml-auto shrink-0 border-transparent bg-transparent text-sidebar-foreground/70 hover:bg-white/10 hover:text-sidebar-foreground",
              sidebarCollapsed && "ml-0"
            )}
            onClick={() => setSidebarCollapsed((value) => !value)}
            title={sidebarCollapsed ? t("Expand sidebar") : t("Collapse sidebar")}
          >
            {sidebarCollapsed ? <PanelLeftOpen size={15} /> : <PanelLeftClose size={15} />}
          </IconButton>
        </div>

        {/* The grouped module nav — the human's jobs are configure, review and
            monitor, and the feature points must be visible on entry. The host
            tree lives inside the Terminal module, which is the connection
            surface. */}
        <nav
          className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto"
          aria-label={t("Modules")}
        >
          {SIDEBAR_GROUPS.map((group, groupIndex) => (
            <div
              key={group.title || `pinned-${groupIndex}`}
              className={cn("flex flex-col gap-0.5", group.pinBottom && "mt-auto pt-2")}
            >
              {group.title && !sidebarCollapsed && (
                <div className="px-2 pt-0.5 text-[10px] font-bold uppercase tracking-wide text-sidebar-foreground/40">
                  {t(group.title)}
                </div>
              )}
              {group.modules.map((id) => (
                <SidebarItem
                  key={id}
                  id={id}
                  active={activeModule === id}
                  collapsed={sidebarCollapsed}
                  pendingCount={pendingApprovals.length}
                  onOpen={openModule}
                />
              ))}
            </div>
          ))}
        </nav>
      </aside>

      {/* The terminal is the connection surface: full-bleed, and kept mounted
          while another module is on screen so its sessions keep running. */}
      <section
        className={cn(
          "min-h-0 min-w-0 flex-1 flex-col",
          activeModule === "terminal" ? "flex" : "hidden",
          !sidebarCollapsed && "max-lg:h-[78vh] max-lg:flex-none"
        )}
      >
        <TerminalPanel
          hosts={hosts}
          groups={groups}
          connectionStatuses={connectionStatuses}
          initialHost={selectedHost}
          focusRequest={terminalFocusRequest}
          onDockAdopted={adoptDock}
          onSessionClosed={releaseDock}
          onNavigateModule={navigateModule}
          onOpenExecute={openHostInExecute}
          onOpenFiles={openHostInFiles}
          onInspectHost={openHostInFleet}
        />
      </section>

      <section
        className={cn(
          "flex min-h-0 min-w-0 flex-1 flex-col gap-3 p-6",
          FULL_HEIGHT_MODULES.has(activeModule) ? "overflow-hidden" : "overflow-y-auto",
          activeModule === "terminal" && "hidden",
          !sidebarCollapsed && "max-lg:overflow-visible"
        )}
      >
        <header className="flex shrink-0 flex-wrap items-center justify-between gap-x-4 gap-y-2">
          <div className="min-w-0">
            <h2 className="flex items-center gap-2 text-xl font-bold">
              <ActiveModuleIcon size={20} className="text-muted-foreground" />
              {t(activeModuleMeta.label)}
            </h2>
            <p className="mt-0.5 text-sm text-muted-foreground">
              {(() => {
                const connectedHosts = connectionStatuses.filter((status) => status.connected).length;
                const hostSummary =
                  activeModule === "hosts" && currentHost
                    ? `${currentHost.user ? `${currentHost.user}@` : ""}${currentHost.host}:${currentHost.port ?? 22}${currentHost.jump_host ? ` via ${currentHost.jump_host}` : ""}${currentHostProxy ? ` · ${t("proxy")} ${currentHostProxy.name}` : ""}`
                    : null;
                const line = moduleSubtitle(activeModule, {
                  pending: pendingApprovals.length,
                  totalHosts: hosts.length,
                  connectedHosts,
                  selectedHost,
                  hostSummary,
                });
                return line.raw ? line.text : t(line.text, line.vars);
              })()}
            </p>
          </div>
          {relatedModules.length > 0 && (
            <RelatedActionsBar
              className="max-w-full justify-end lg:max-w-[58%]"
              current={activeModuleMeta}
              related={relatedModules}
              onNavigate={navigateModule}
            />
          )}
        </header>

        <section
          className={cn(
            "module-page",
            FULL_HEIGHT_MODULES.has(activeModule) && "flex min-h-0 flex-1 flex-col",
          )}
        >
        {activeModule === "dashboard" && (
          <Dashboard
            hosts={hosts}
            connectionStatuses={connectionStatuses}
            pendingApprovalsCount={pendingApprovals.length}
            secretsLocked={secretsLocked}
            daemonHealth={daemonHealth}
            onNavigateModule={navigateModule}
            onRequestUnlock={requestCredentialUnlock}
            onStartDaemon={startDaemonFromDashboard}
            onOpenAnomalies={openAnomalies}
            onOpenHostTerminal={openHostInTerminal}
            onInspectHost={openHostInFleet}
          />
        )}

        {activeModule === "hosts" && (
          <div className="space-y-4">
            <LocalEnvironmentsCard onExecuteLocal={openHostInExecute} />
            <HostList
              hosts={hosts}
              groups={groups}
              proxies={proxies}
              selectedHost={selectedHost}
              selectedGroup={selectedGroup}
              connectionStatuses={connectionStatuses}
              connectionProgress={connectionProgress}
              highlightHost={highlightedHost}
              onSelect={setSelectedHost}
              onGroupSelect={setSelectedGroup}
              onCreateGroup={handleCreateGroup}
              onRenameGroup={handleRenameGroup}
              onDeleteGroup={handleDeleteGroup}
              onOpenAddHost={openAddHostDrawer}
              onOpenTerminal={openHostInTerminal}
              onOpenExecute={openHostInExecute}
              onOpenFiles={openHostInFiles}
              onEdit={openEditHostDrawer}
              onRemove={handleRemoveHost}
              onBatchRemove={handleBatchRemoveHosts}
              onRefresh={refresh}
              onConnect={handleConnect}
              onDisconnect={handleDisconnect}
              onImportOpensshTrust={handleImportOpensshTrust}
              onForgetOpenssh={handleForgetOpenssh}
              onImportNetcatty={openNetcattyImport}
            />
            <HostDrawer
              open={isHostDrawerOpen}
              editingHostName={editingHost?.name ?? null}
              onClose={closeHostDrawer}
            >
              <AddHostForm
                hosts={hosts}
                groups={groups}
                proxies={proxies}
                initialGroup={selectedGroup === ALL_GROUPS_ID ? "default" : selectedGroup}
                editingHost={editingHost}
                onCancelEdit={closeHostDrawer}
                onSaved={handleHostSaved}
                onImportNetcatty={openNetcattyImport}
              />
            </HostDrawer>
          </div>
        )}

        {activeModule === "proxies" && (
          <ProxyPanel
            proxies={proxies}
            hosts={hosts}
            onChanged={handleProxyChanged}
            onSelectHost={openHostInFleet}
          />
        )}

        {activeModule === "recordings" && <RecordingsPanel />}

        {(activeModule === "execute" || activeModule === "playbooks") && (
          <ExecutionWorkbench
            hosts={hosts}
            initialHost={selectedHost}
            initialCommand={execInitialCommand}
            carryEpoch={execCarryEpoch}
            preferredMode={activeModule === "playbooks" ? "playbooks" : execPreferredMode}
            onPreferredModeApplied={activeModule === "playbooks" ? undefined : clearExecPreferredMode}
            onModeChange={handleExecModeChange}
            onExecComplete={handleExecComplete}
          />
        )}

        {activeModule === "files-sessions" && (
          <SFTPPanel hosts={hosts} initialHost={selectedHost} />
        )}

        {activeModule === "tunnels" && (
          <ForwardPanel
            hosts={hosts}
            initialHost={selectedHost}
            onChanged={handleForwardChanged}
            onSelectHost={openHostInFleet}
          />
        )}

        {activeModule === "activity" && (
          <LiveActivityPanel presetFilter={activityPreset} presetEpoch={activityPresetEpoch} />
        )}

        {activeModule === "mcp-agents" && <McpAgentsPanel />}

        {activeModule === "sync" && <SyncPanel onRestored={refresh} />}

        {activeModule === "keys" && (
          <KeysPanel hosts={hosts} onChanged={handleKeyChanged} onSelectHost={openHostInFleet} />
        )}

        {activeModule === "audit" && (
          <AuditWorkbench
            onReplay={replayAuditEntry}
            onEditAndExecute={goToExecuteFromAudit}
          />
        )}

        {activeModule === "help" && <HelpPanel />}

        {activeModule === "approvals" && (
          <ApprovalWorkbench
            pending={pendingApprovals}
            focusRequest={approvalFocusRequest}
            onApprove={handleApprove}
            onReject={handleReject}
            onGoExecute={goToExecuteForApproval}
          />
        )}

        {activeModule === "config" && <ConfigSnapshotsPanel />}

        {activeModule === "containers" && <ContainersPanel />}
        {activeModule === "resident-sessions" && <ResidentSessionsPanel />}

        {activeModule === "topology" && (
          <ConnectionTopology
            hosts={hosts}
            proxies={proxies}
            connectionStatuses={connectionStatuses}
            daemonHealth={daemonHealth}
            onSelectHost={openHostInFleet}
            onNavigateModule={navigateModule}
            onOpenTerminal={openHostInTerminal}
            onOpenExecute={openHostInExecute}
          />
        )}
      </section>
      </section>
      </main>

      {/* v3 §3.4: terminal drawer — approvals + audit replay viewport */}
      <TerminalDrawer
        open={terminalDrawer !== null}
        host={terminalDrawer?.host ?? ""}
        command={terminalDrawer?.command ?? null}
        slotId={terminalDrawer ? `drawer-${terminalDrawer.id}` : null}
        connected={drawerConnected}
        onClose={closeTerminalDrawer}
        onOpenInTerminal={openDrawerInTerminal}
      />
      {dockedPtys.map((item) => (
        <DockedView
          key={item.id}
          slotId={item.sessionId ? `session-${item.sessionId}` : `drawer-${item.id}`}
        >
          <LiveTerminal
            host={item.host}
            command={item.command}
            onConnectedChange={(connected) => {
              if (terminalDrawer?.id === item.id) setDrawerConnected(connected);
            }}
          />
        </DockedView>
      ))}

      <ConfirmHost />
      <NetcattyImportDialog
        open={netcattyOpen}
        onClose={closeNetcattyImport}
        onImported={refresh}
      />
    </div>
    </TerminalDock>
  );
}
