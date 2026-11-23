import {
  Columns2,
  Copy,
  History,
  LayoutGrid,
  Rows2,
  Search,
  Square,
  TerminalSquare,
} from "lucide-react";
import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { useI18n } from "../i18n";
import { api } from "../api";
import type { ModuleId } from "../nav";
import type { ConnectionStatus, HostGroup, HostProfile, TerminalBroadcastResponse } from "../types";
import {
  isTerminalThemeId,
  TERMINAL_THEME_OPTIONS,
  TERMINAL_THEME_STORAGE_KEY,
  terminalThemeBackground,
  type TerminalThemeId,
} from "../terminalThemes";
import { useTheme, type Theme as AppTheme } from "../theme";
import TerminalView, { type TerminalViewHandle } from "./TerminalView";
import SnippetsDialog from "./SnippetsDialog";
import SFTPPanel from "./SFTPPanel";
import { TerminalSlot } from "../features/terminal/dock";
import HostTree from "../features/terminal/HostTree";
import SessionTabs from "../features/terminal/SessionTabs";
import {
  activateSession,
  assignPane,
  closeSession,
  focusOrOpenSession,
  loadTerminalState,
  moveSession,
  openNewSession,
  paneCount,
  paneRect,
  pruneMissingHosts,
  saveTerminalState,
  sessionLabels,
  setLayout,
  setFilesRatio,
  setRatios,
  type PaneLayout,
  type TerminalState,
} from "../features/terminal/sessions";
import { Button } from "./ui/button";
import { IconButton } from "./ui/icon-button";
import { Input } from "./ui/input";
import { Select } from "./ui/select";
import { Textarea } from "./ui/textarea";
import { Dialog } from "./ui/dialog";
import RiskBadge from "./RiskBadge";
import { cn } from "../lib/utils";
import type { CommandBlockMetadata } from "../lib/terminal/block-content";

type Props = {
  hosts: HostProfile[];
  groups: HostGroup[];
  connectionStatuses: ConnectionStatus[];
  /** Host selected elsewhere in the console; the strip's "+" opens a session to it. */
  initialHost?: string;
  /**
   * Drawer handoff. `dockId` names the already-mounted PTY. The panel focuses
   * or creates the host's tab and renders a slot there instead of a second
   * TerminalView, so the same socket moves in. No command is replayed.
   */
  focusRequest?: { host: string; dockId: string | null; nonce: number } | null;
  /** Called with the tab that now owns `dockId`, once that tab exists. */
  onDockAdopted?: (dockId: string, sessionId: string) => void;
  /** The docked PTY should close when its tab does. */
  onSessionClosed?: (sessionId: string) => void;
  onNavigateModule?: (id: ModuleId) => void;
};

const PANE_HEADER_PX = 28;
const HISTORY_MAX_ENTRIES = 200;

function initialTerminalTheme(): TerminalThemeId {
  try {
    const saved = localStorage.getItem(TERMINAL_THEME_STORAGE_KEY);
    if (saved && isTerminalThemeId(saved)) return saved;
  } catch {
    // localStorage may be unavailable
  }
  return "app";
}

function systemTheme(): AppTheme {
  if (typeof window === "undefined") return "dark";
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

/**
 * The terminal surface: host tree → sessions → panes, with the focused
 * session's host owning the file drawer below. Split panes, broadcast, command
 * blocks, Ctrl+R history, snippets and the terminal theme all act on these
 * sessions, so every tool shares one connection model.
 */
function TerminalPanel({
  hosts,
  groups,
  connectionStatuses,
  initialHost = "",
  focusRequest = null,
  onDockAdopted,
  onSessionClosed,
  onNavigateModule,
}: Props) {
  const { t } = useI18n();
  const { theme: appTheme } = useTheme();
  const [state, setState] = useState<TerminalState>(() => loadTerminalState());
  const stateRef = useRef(state);
  stateRef.current = state;
  const [focusMode, setFocusMode] = useState(false);
  const [terminalTheme, setTerminalTheme] = useState<TerminalThemeId>(() => initialTerminalTheme());
  const [resolvedSystemTheme, setResolvedSystemTheme] = useState<AppTheme>(() => systemTheme());

  const { sessions, panes, layout, focusedPane, ratios, filesOpen, filesRatio } = state;
  const focusedSessionId = panes[focusedPane] ?? null;
  const focusedSession = sessions.find((session) => session.id === focusedSessionId) ?? null;

  const containerRef = useRef<HTMLDivElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  // Per-session typed-line history (Ctrl+R) and the handles used to inject a
  // picked entry back into the right pane.
  const historyRef = useRef<Map<string, string[]>>(new Map());
  const terminalRefs = useRef<Map<string, TerminalViewHandle>>(new Map());
  const focusedSessionIdRef = useRef<string | null>(null);
  focusedSessionIdRef.current = focusedSessionId;

  const [historySearch, setHistorySearch] = useState<{ paneIndex: number; query: string } | null>(
    null,
  );
  const [snippetsOpen, setSnippetsOpen] = useState(false);
  const [blocksBySession, setBlocksBySession] = useState<Map<string, CommandBlockMetadata[]>>(
    () => new Map(),
  );
  const [blockQuery, setBlockQuery] = useState("");
  const [blockMatches, setBlockMatches] = useState<CommandBlockMetadata[]>([]);
  const [selectedBlock, setSelectedBlock] = useState<{
    sessionId: string;
    block: CommandBlockMetadata;
  } | null>(null);
  const [blockCopyStatus, setBlockCopyStatus] = useState<"idle" | "copied" | "failed">("idle");
  const [terminalConnections, setTerminalConnections] = useState<Map<string, string>>(
    () => new Map(),
  );
  const [cwdBySession, setCwdBySession] = useState<Map<string, string>>(() => new Map());
  // Sessions whose file drawer has been opened stay mounted (hidden) so their
  // browse paths and selection survive switching panes.
  const [drawerSessions, setDrawerSessions] = useState<string[]>([]);
  const [broadcastOpen, setBroadcastOpen] = useState(false);
  const [broadcastTargets, setBroadcastTargets] = useState<Set<string>>(() => new Set());
  const [broadcastCommand, setBroadcastCommand] = useState("");
  const [broadcastForce, setBroadcastForce] = useState(false);
  const [broadcastPreview, setBroadcastPreview] = useState<TerminalBroadcastResponse | null>(null);
  const [broadcastBusy, setBroadcastBusy] = useState(false);
  const [broadcastError, setBroadcastError] = useState<string | null>(null);

  const labels = useMemo(() => sessionLabels(sessions), [sessions]);
  const sessionCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const session of sessions) counts.set(session.host, (counts.get(session.host) ?? 0) + 1);
    return counts;
  }, [sessions]);
  const connectedHosts = useMemo(
    () => new Set(connectionStatuses.filter((status) => status.connected).map((status) => status.host)),
    [connectionStatuses],
  );

  useEffect(() => {
    saveTerminalState(state);
  }, [state]);

  // Sessions whose pane shows the drawer's PTY instead of a new TerminalView.
  const [dockedSessionIds, setDockedSessionIds] = useState<string[]>([]);
  const onDockAdoptedRef = useRef(onDockAdopted);
  onDockAdoptedRef.current = onDockAdopted;

  // Drawer → main terminal. Focus or create the host tab, and if this handoff
  // carries a dock id, that tab renders a slot for the already-open PTY.
  // The command is not written here: null was already run by the daemon, and
  // a non-null command was already typed in the drawer.
  useEffect(() => {
    if (!focusRequest?.host) return;
    const next = focusOrOpenSession(stateRef.current, focusRequest.host);
    const sessionId =
      [...next.sessions].reverse().find((session) => session.host === focusRequest.host)?.id ?? "";
    setState(next);
    if (focusRequest.dockId && sessionId) {
      setDockedSessionIds((ids) => (ids.includes(sessionId) ? ids : [...ids, sessionId]));
      onDockAdoptedRef.current?.(focusRequest.dockId, sessionId);
    }
  }, [focusRequest]);

  // A host removed from the configuration takes its sessions and panes with it.
  useEffect(() => {
    const names = new Set(hosts.map((host) => host.name));
    setState((current) => pruneMissingHosts(current, names));
  }, [hosts]);

  useEffect(() => {
    setDrawerSessions((previous) => {
      const alive = previous.filter((id) => sessions.some((session) => session.id === id));
      return alive.length === previous.length ? previous : alive;
    });
  }, [sessions]);

  useEffect(() => {
    if (!filesOpen || !focusedSessionId) return;
    setDrawerSessions((previous) =>
      previous.includes(focusedSessionId) ? previous : [...previous, focusedSessionId],
    );
  }, [filesOpen, focusedSessionId]);

  useEffect(() => {
    try {
      localStorage.setItem(TERMINAL_THEME_STORAGE_KEY, terminalTheme);
    } catch {
      // ignore persistence failures
    }
  }, [terminalTheme]);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: light)");
    const update = () => setResolvedSystemTheme(systemTheme());
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);

  const effectiveAppTheme = appTheme === "system" ? resolvedSystemTheme : appTheme;
  const terminalBackground = useMemo(
    () => terminalThemeBackground(terminalTheme, effectiveAppTheme),
    [effectiveAppTheme, terminalTheme],
  );

  const insertSnippet = useCallback((command: string) => {
    const sessionId = focusedSessionIdRef.current;
    if (!sessionId) return;
    const terminal = terminalRefs.current.get(sessionId);
    if (!terminal) return;
    terminal.sendText(command);
    terminal.focus();
    setSnippetsOpen(false);
  }, []);

  function openHost(host: string) {
    setState((current) => focusOrOpenSession(current, host));
  }

  function newSession(host: string) {
    setState((current) => openNewSession(current, host));
  }

  function close(id: string) {
    setDockedSessionIds((ids) => (ids.includes(id) ? ids.filter((item) => item !== id) : ids));
    onSessionClosed?.(id);
    historyRef.current.delete(id);
    terminalRefs.current.delete(id);
    setTerminalConnections((previous) => {
      const next = new Map(previous);
      next.delete(id);
      return next;
    });
    setBroadcastTargets((previous) => {
      const next = new Set(previous);
      next.delete(id);
      return next;
    });
    setBlocksBySession((previous) => {
      const next = new Map(previous);
      next.delete(id);
      return next;
    });
    setSelectedBlock((previous) => (previous?.sessionId === id ? null : previous));
    setCwdBySession((previous) => {
      const next = new Map(previous);
      next.delete(id);
      return next;
    });
    setState((current) => closeSession(current, id));
  }

  function toggleFiles() {
    setState((current) => ({ ...current, filesOpen: !current.filesOpen }));
  }

  function openBroadcast() {
    const connected = sessions.filter((session) => terminalConnections.has(session.id));
    setBroadcastTargets(new Set(connected.map((session) => session.id)));
    const selection = focusedSessionId
      ? terminalRefs.current.get(focusedSessionId)?.getSelection().trim()
      : "";
    setBroadcastCommand(selection && !selection.includes("\n") ? selection : "");
    setBroadcastForce(false);
    setBroadcastPreview(null);
    setBroadcastError(null);
    setBroadcastOpen(true);
  }

  const broadcastRequest = () => ({
    targets: sessions
      .filter((session) => broadcastTargets.has(session.id))
      .flatMap((session) => {
        const terminalId = terminalConnections.get(session.id);
        return terminalId ? [{ terminal_id: terminalId, host: session.host }] : [];
      }),
    command: broadcastCommand.trim(),
    force: broadcastForce,
    all_or_nothing: true,
  });

  async function previewBroadcast() {
    setBroadcastBusy(true);
    setBroadcastError(null);
    try {
      setBroadcastPreview(await api.terminalBroadcastPreview(broadcastRequest()));
    } catch (error) {
      setBroadcastPreview(null);
      setBroadcastError(String(error));
    } finally {
      setBroadcastBusy(false);
    }
  }

  async function runBroadcast() {
    setBroadcastBusy(true);
    setBroadcastError(null);
    try {
      const result = await api.terminalBroadcastRun(broadcastRequest());
      setBroadcastPreview(result);
      if (result.enqueued_any) {
        setBroadcastOpen(false);
      } else {
        setBroadcastError(t("Broadcast was not sent to any terminal"));
      }
    } catch (error) {
      setBroadcastError(String(error));
    } finally {
      setBroadcastBusy(false);
    }
  }

  function updateBlockSearch(sessionId: string | null, query: string) {
    if (!sessionId) {
      setBlockMatches([]);
      return;
    }
    const handle = terminalRefs.current.get(sessionId);
    setBlockMatches(handle ? handle.searchBlocks(query) : (blocksBySession.get(sessionId) ?? []));
  }

  function selectCommandBlock(sessionId: string, block: CommandBlockMetadata) {
    if (!terminalRefs.current.get(sessionId)?.selectBlock(block.id)) return;
    setSelectedBlock({ sessionId, block });
    setBlockCopyStatus("idle");
  }

  async function copySelectedCommandBlock() {
    if (!selectedBlock) return;
    const result = await terminalRefs.current
      .get(selectedBlock.sessionId)
      ?.copyBlock(selectedBlock.block.id);
    setBlockCopyStatus(result?.ok ? "copied" : "failed");
  }

  function recordLine(sessionId: string, line: string) {
    const list = historyRef.current.get(sessionId) ?? [];
    const deduped = list.filter((entry) => entry !== line);
    deduped.unshift(line);
    historyRef.current.set(sessionId, deduped.slice(0, HISTORY_MAX_ENTRIES));
  }

  function pickHistoryEntry(paneIndex: number, sessionId: string, text: string) {
    terminalRefs.current.get(sessionId)?.sendText(text);
    setHistorySearch(null);
    terminalRefs.current.get(sessionId)?.focus();
    setState((current) => ({ ...current, focusedPane: paneIndex }));
  }

  function startDrag(axis: "col" | "row", event: ReactPointerEvent) {
    event.preventDefault();
    const container = containerRef.current;
    if (!container) return;
    const rect = container.getBoundingClientRect();
    function onMove(moveEvent: PointerEvent) {
      const fraction =
        axis === "col"
          ? (moveEvent.clientX - rect.left) / rect.width
          : (moveEvent.clientY - rect.top) / rect.height;
      const clamped = Math.min(0.85, Math.max(0.15, fraction));
      setState((current) =>
        setRatios(
          current,
          axis === "col"
            ? { ...current.ratios, col: clamped }
            : { ...current.ratios, row: clamped },
        ),
      );
    }
    function onUp() {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    }
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }

  /** The drawer is measured from the bottom of the stage, not the top. */
  function startFilesDrag(event: ReactPointerEvent) {
    event.preventDefault();
    const stage = stageRef.current;
    if (!stage) return;
    const rect = stage.getBoundingClientRect();
    function onMove(moveEvent: PointerEvent) {
      // A stage hidden mid-drag measures zero; keep the last usable ratio.
      if (rect.height <= 0) return;
      setState((current) => setFilesRatio(current, (rect.bottom - moveEvent.clientY) / rect.height));
    }
    function onUp() {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    }
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }

  const visiblePanes = Array.from({ length: paneCount(layout) }, (_, index) => index);
  const searchSessionId = historySearch ? panes[historySearch.paneIndex] : null;
  const searchRect = historySearch ? paneRect(layout, ratios, historySearch.paneIndex) : null;
  const searchResults = useMemo(() => {
    if (!historySearch || !searchSessionId) return [];
    const all = historyRef.current.get(searchSessionId) ?? [];
    const needle = historySearch.query.trim().toLowerCase();
    return needle ? all.filter((line) => line.toLowerCase().includes(needle)) : all;
  }, [historySearch, searchSessionId]);
  const visibleBlocks = blockQuery.trim()
    ? blockMatches
    : focusedSessionId
      ? (blocksBySession.get(focusedSessionId) ?? [])
      : [];
  const mountedDrawers = drawerSessions.filter((id) =>
    sessions.some((session) => session.id === id),
  );

  useEffect(() => {
    updateBlockSearch(focusedSessionId, blockQuery);
  }, [focusedSessionId]);

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-1 overflow-hidden">
      {!focusMode && (
        <div className="flex w-[240px] shrink-0 flex-col gap-3 overflow-y-auto border-r border-line bg-muted/30 p-2.5 max-lg:w-[190px]">
          <HostTree
            hosts={hosts}
            groups={groups}
            connectionStatuses={connectionStatuses}
            sessionCounts={sessionCounts}
            onOpenHost={openHost}
            onNewSession={newSession}
          />

          <div className="grid shrink-0 gap-1.5 border-t border-line pt-2">
            <div className="px-1 text-[11px] font-bold uppercase tracking-wide text-muted-foreground">
              {t("Split view")}
            </div>
            <div className="grid grid-cols-4 gap-1">
              <IconButton
                size="sm"
                variant={layout === "single" ? "default" : "ghost"}
                title={t("Single pane")}
                onClick={() => setState((current) => setLayout(current, "single"))}
                className={layout === "single" ? "border-primary/40 bg-primary/10 text-primary" : ""}
              >
                <Square size={14} />
              </IconButton>
              <IconButton
                size="sm"
                variant={layout === "row-2" ? "default" : "ghost"}
                title={t("Split horizontally")}
                onClick={() => setState((current) => setLayout(current, "row-2"))}
                className={layout === "row-2" ? "border-primary/40 bg-primary/10 text-primary" : ""}
              >
                <Columns2 size={14} />
              </IconButton>
              <IconButton
                size="sm"
                variant={layout === "col-2" ? "default" : "ghost"}
                title={t("Split vertically")}
                onClick={() => setState((current) => setLayout(current, "col-2"))}
                className={layout === "col-2" ? "border-primary/40 bg-primary/10 text-primary" : ""}
              >
                <Rows2 size={14} />
              </IconButton>
              <IconButton
                size="sm"
                variant={layout === "grid-4" ? "default" : "ghost"}
                title={t("Split into 4")}
                onClick={() => setState((current) => setLayout(current, "grid-4"))}
                className={layout === "grid-4" ? "border-primary/40 bg-primary/10 text-primary" : ""}
              >
                <LayoutGrid size={14} />
              </IconButton>
            </div>
          </div>

          <div className="grid max-h-[210px] shrink-0 gap-1.5 border-t border-line pt-2">
            <div className="px-1 text-[11px] font-bold uppercase tracking-wide text-muted-foreground">
              {t("Command blocks")}
            </div>
            <div className="relative">
              <Search
                size={12}
                className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-muted-foreground"
              />
              <Input
                value={blockQuery}
                onChange={(event) => {
                  const query = event.target.value;
                  setBlockQuery(query);
                  updateBlockSearch(focusedSessionId, query);
                }}
                placeholder={t("Search blocks")}
                disabled={!focusedSessionId}
                className="h-7 pl-7 text-xs"
              />
            </div>
            <div className="max-h-[92px] overflow-y-auto rounded border border-border/70">
              {visibleBlocks.length === 0 ? (
                <div className="px-2 py-3 text-center text-[11px] text-muted-foreground">
                  {t(focusedSessionId ? "No command blocks" : "Select a session")}
                </div>
              ) : (
                visibleBlocks
                  .slice()
                  .reverse()
                  .slice(0, 50)
                  .map((block) => (
                    <button
                      key={block.id}
                      type="button"
                      onClick={() => focusedSessionId && selectCommandBlock(focusedSessionId, block)}
                      className={cn(
                        "flex w-full items-center gap-1.5 border-b border-border/50 px-2 py-1 text-left text-[11px] last:border-b-0 hover:bg-muted",
                        selectedBlock?.sessionId === focusedSessionId &&
                          selectedBlock.block.id === block.id
                          ? "bg-primary/10 text-primary"
                          : "",
                      )}
                      title={block.command ?? t("Command unavailable")}
                    >
                      <span
                        className="h-3 w-1 shrink-0 rounded-full"
                        style={{ backgroundColor: block.color }}
                      />
                      <span className="truncate font-mono">
                        {block.command ?? t("Command unavailable")}
                      </span>
                      <span className="ml-auto shrink-0 text-[9px] text-muted-foreground">
                        {block.startLine + 1}-{block.endLine + 1}
                      </span>
                    </button>
                  ))
              )}
            </div>
            {selectedBlock && selectedBlock.sessionId === focusedSessionId ? (
              <div className="flex items-center gap-1.5 px-1 text-[10px] text-muted-foreground">
                <span className="min-w-0 flex-1 truncate" title={selectedBlock.block.startedAt}>
                  {selectedBlock.block.active ? t("Running") : t("Completed")} ·{" "}
                  {selectedBlock.block.host}
                </span>
                <Button
                  size="sm"
                  variant="secondary"
                  className="h-6 px-2 text-[10px]"
                  onClick={() => void copySelectedCommandBlock()}
                >
                  <Copy size={11} />
                  {t(
                    blockCopyStatus === "copied"
                      ? "Copied"
                      : blockCopyStatus === "failed"
                        ? "Copy failed"
                        : "Copy",
                  )}
                </Button>
              </div>
            ) : null}
          </div>

          <label className="grid shrink-0 gap-1 text-xs font-medium text-muted-foreground">
            <span>{t("Terminal theme")}</span>
            <Select
              value={terminalTheme}
              onChange={(event) => {
                const next = event.target.value;
                if (isTerminalThemeId(next)) setTerminalTheme(next);
              }}
              className="h-8 text-xs"
            >
              {TERMINAL_THEME_OPTIONS.map((option) => (
                <option key={option.id} value={option.id}>
                  {t(option.label)}
                </option>
              ))}
            </Select>
          </label>

          {/* Keeps the tools packed under the tree instead of stranded at the
              bottom of a tall, mostly empty column. */}
          <div className="min-h-0 flex-1" />
        </div>
      )}

      <div ref={stageRef} className="flex min-h-0 min-w-0 flex-1 flex-col">
        <SessionTabs
          sessions={sessions}
          panes={panes}
          connectedHosts={connectedHosts}
          focusMode={focusMode}
          filesOpen={filesOpen}
          canBroadcast={terminalConnections.size >= 2}
          onActivate={(id) => setState((current) => activateSession(current, id))}
          onClose={close}
          onReorder={(dragId, overId) => setState((current) => moveSession(current, dragId, overId))}
          onNewSession={() => newSession(initialHost || hosts[0]?.name || "")}
          onOpenSnippets={() => setSnippetsOpen(true)}
          onOpenBroadcast={openBroadcast}
          onToggleFiles={toggleFiles}
          onToggleFocusMode={() => setFocusMode((mode) => !mode)}
        />

        {/* Pane grid */}
        <div
          ref={containerRef}
          className="relative min-h-0 min-w-0 flex-1"
          style={{ backgroundColor: terminalBackground }}
        >
          {sessions.length === 0 && (
            <div className="absolute inset-0 flex items-center justify-center p-6">
              <div className="grid w-full max-w-lg gap-4 rounded-2xl border border-white/10 bg-black/25 p-6">
                <div className="flex items-center gap-3">
                  <span className="grid size-9 shrink-0 place-items-center rounded-lg border border-primary/40 bg-primary/15 text-primary">
                    <TerminalSquare size={18} />
                  </span>
                  <div className="grid gap-0.5">
                    <div className="text-sm font-semibold text-white/90">{t("Terminal")}</div>
                    <div className="text-xs text-white/55">
                      {hosts.length === 0
                        ? t("Add a host to start issuing SSH commands")
                        : t("Pick a host on the left to open its terminal and files.")}
                    </div>
                  </div>
                </div>
                {hosts.length === 0
                  ? onNavigateModule && (
                      <Button size="sm" variant="secondary" onClick={() => onNavigateModule("hosts")}>
                        {t("New host")}
                      </Button>
                    )
                  : (
                    <div className="grid gap-1.5">
                      <div className="text-[11px] font-bold uppercase tracking-wide text-white/45">
                        {t("Open a session")}
                      </div>
                      <div className="grid gap-1 sm:grid-cols-2">
                        {hosts.slice(0, 6).map((host) => (
                          <button
                            key={host.name}
                            type="button"
                            onClick={() => openHost(host.name)}
                            className="flex items-center gap-2 rounded-lg border border-white/10 bg-white/5 px-2.5 py-2 text-left text-xs text-white/85 transition-colors hover:border-primary/50 hover:bg-primary/15 hover:text-white"
                          >
                            <span
                              className={cn(
                                "size-1.5 shrink-0 rounded-full",
                                connectedHosts.has(host.name) ? "bg-success" : "bg-risk-medium/70",
                              )}
                            />
                            <span className="min-w-0 truncate">{host.name}</span>
                            <span className="ml-auto shrink-0 font-mono text-[10px] text-white/40">
                              {host.host}
                            </span>
                          </button>
                        ))}
                      </div>
                    </div>
                  )}
                <div className="text-[11px] text-white/40">
                  {t("Split panes, broadcast and the file drawer all act on these sessions.")}
                </div>
              </div>
            </div>
          )}

          {/* Pane chrome: header + border + focus ring, one per visible pane slot. */}
          {sessions.length > 0 && visiblePanes.map((paneIndex) => {
            const rect = paneRect(layout, ratios, paneIndex);
            const sessionId = panes[paneIndex];
            const session = sessions.find((candidate) => candidate.id === sessionId);
            return (
              <div
                key={`chrome-${paneIndex}`}
                className={cn(
                  "pointer-events-none absolute z-[2] border",
                  paneIndex === focusedPane ? "border-primary/60" : "border-white/10",
                )}
                style={{
                  top: `${rect.top}%`,
                  left: `${rect.left}%`,
                  width: `${rect.width}%`,
                  height: `${rect.height}%`,
                }}
              >
                <div
                  className="pointer-events-auto flex items-center gap-1.5 bg-black/30 px-2 text-[11px] text-white/70"
                  style={{ height: PANE_HEADER_PX }}
                  onClick={() => setState((current) => ({ ...current, focusedPane: paneIndex }))}
                >
                  <TerminalSquare size={11} className="shrink-0" />
                  <Select
                    value={sessionId ?? ""}
                    aria-label={t("Pane {index} session", { index: paneIndex + 1 })}
                    onChange={(event) =>
                      setState((current) => assignPane(current, paneIndex, event.target.value || null))
                    }
                    className="h-6 max-w-[160px] border-white/15 bg-transparent px-1 text-[11px] text-white/80"
                  >
                    <option value="">{t("Select a session")}</option>
                    {sessions.map((candidate) => (
                      <option key={candidate.id} value={candidate.id}>
                        {labels.get(candidate.id) ?? candidate.host}
                      </option>
                    ))}
                  </Select>
                  <IconButton
                    size="sm"
                    title={t("Search command history")}
                    onClick={() => setHistorySearch({ paneIndex, query: "" })}
                    disabled={!sessionId}
                    className="ml-auto h-6 w-6 border-white/15 bg-transparent text-white/70 hover:text-white"
                  >
                    <History size={12} />
                  </IconButton>
                </div>
                {!session && (
                  <div className="pointer-events-none flex h-[calc(100%-28px)] items-center justify-center text-xs text-white/40">
                    {t("Pick a session above")}
                  </div>
                )}
              </div>
            );
          })}

          {/* Resize handles. */}
          {sessions.length > 0 && (layout === "row-2" || layout === "grid-4") && (
            <div
              className="absolute inset-y-0 z-10 w-1.5 -translate-x-1/2 cursor-col-resize hover:bg-primary/40"
              style={{ left: `${ratios.col * 100}%` }}
              onPointerDown={(event) => startDrag("col", event)}
            />
          )}
          {sessions.length > 0 && (layout === "col-2" || layout === "grid-4") && (
            <div
              className="absolute inset-x-0 z-10 h-1.5 -translate-y-1/2 cursor-row-resize hover:bg-primary/40"
              style={{ top: `${ratios.row * 100}%` }}
              onPointerDown={(event) => startDrag("row", event)}
            />
          )}

          {/* Live terminals: every open session stays mounted (background
              sessions keep running) but only positioned into view when assigned
              to a pane. */}
          {sessions.map((session) => {
            const paneIndex = panes.indexOf(session.id);
            const assigned = paneIndex >= 0;
            const rect = assigned ? paneRect(layout, ratios, paneIndex) : null;
            const cwd = cwdBySession.get(session.id);
            return (
              <div
                key={session.id}
                data-session-id={session.id}
                className="absolute"
                style={
                  assigned && rect
                    ? {
                        visibility: "visible",
                        top: `calc(${rect.top}% + ${PANE_HEADER_PX}px)`,
                        left: `${rect.left}%`,
                        width: `${rect.width}%`,
                        height: `calc(${rect.height}% - ${PANE_HEADER_PX}px)`,
                        zIndex: 1,
                      }
                    : { visibility: "hidden", top: 0, left: 0, width: "100%", height: "100%", zIndex: 0 }
                }
              >
                {dockedSessionIds.includes(session.id) ? (
                  <TerminalSlot id={`session-${session.id}`} className="h-full" />
                ) : (
                <TerminalView
                  ref={(handle) => {
                    if (handle) terminalRefs.current.set(session.id, handle);
                    else terminalRefs.current.delete(session.id);
                  }}
                  host={session.host}
                  terminalTheme={terminalTheme}
                  appTheme={effectiveAppTheme}
                  onLineTyped={(line) => recordLine(session.id, line)}
                  onHistoryRequest={
                    assigned ? () => setHistorySearch({ paneIndex, query: "" }) : undefined
                  }
                  onOsc7Cwd={(next) =>
                    setCwdBySession((previous) => {
                      const map = new Map(previous);
                      map.set(session.id, next);
                      return map;
                    })
                  }
                  onBlocksChange={(blocks) => {
                    setBlocksBySession((previous) => {
                      const next = new Map(previous);
                      next.set(session.id, blocks);
                      return next;
                    });
                    if (focusedSessionIdRef.current === session.id && blockQuery.trim()) {
                      queueMicrotask(() => updateBlockSearch(session.id, blockQuery));
                    }
                    setSelectedBlock((previous) => {
                      if (previous?.sessionId !== session.id) return previous;
                      const updated = blocks.find((block) => block.id === previous.block.id);
                      return updated ? { sessionId: session.id, block: updated } : null;
                    });
                  }}
                  onBlockSelected={(block) => {
                    setSelectedBlock(block ? { sessionId: session.id, block } : null);
                    setBlockCopyStatus("idle");
                  }}
                  onConnectionChange={(connection) => {
                    setTerminalConnections((previous) => {
                      const next = new Map(previous);
                      if (connection) next.set(session.id, connection.terminalId);
                      else next.delete(session.id);
                      return next;
                    });
                  }}
                />
                )}
                {/* OSC 7 cwd chip — best-effort: only shells with integration report it. */}
                {cwd && (
                  <span
                    className="absolute bottom-2 left-2 z-[4] max-w-56 truncate rounded-lg border border-edge bg-popover/90 px-2 py-1 font-mono text-[11px] text-popover-foreground shadow"
                    title={cwd}
                  >
                    {cwd}
                  </span>
                )}
              </div>
            );
          })}

          {historySearch && searchSessionId && searchRect && (
            <div
              className="absolute z-20 rounded-lg border border-edge bg-popover text-popover-foreground"
              style={{
                top: `calc(${searchRect.top}% + ${PANE_HEADER_PX + 6}px)`,
                left: `calc(${searchRect.left}% + 8px)`,
                width: `min(420px, ${searchRect.width}% - 16px)`,
              }}
            >
              <div className="flex items-center gap-2 border-b border-border px-3 py-2">
                <History size={14} className="shrink-0 text-muted-foreground" />
                <Input
                  autoFocus
                  value={historySearch.query}
                  onChange={(event) => setHistorySearch({ ...historySearch, query: event.target.value })}
                  onKeyDown={(event) => {
                    if (event.key === "Escape") {
                      event.preventDefault();
                      setHistorySearch(null);
                    } else if (event.key === "Enter" && searchResults[0]) {
                      event.preventDefault();
                      pickHistoryEntry(historySearch.paneIndex, searchSessionId, searchResults[0]);
                    }
                  }}
                  placeholder={t("Search command history")}
                  className="h-7 border-none bg-transparent px-0 shadow-none focus-visible:ring-0"
                />
                <kbd className="shrink-0 rounded border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground">
                  Esc
                </kbd>
              </div>
              <div className="max-h-[240px] overflow-y-auto py-1">
                {searchResults.length === 0 && (
                  <div className="px-3 py-4 text-center text-xs text-muted-foreground">
                    {t("No matches")}
                  </div>
                )}
                {searchResults.slice(0, 30).map((line, index) => (
                  <button
                    key={`${line}-${index}`}
                    type="button"
                    onClick={() => pickHistoryEntry(historySearch.paneIndex, searchSessionId, line)}
                    className="block w-full truncate px-3 py-1.5 text-left font-mono text-xs hover:bg-muted"
                    title={line}
                  >
                    {line}
                  </button>
                ))}
              </div>
            </div>
          )}
        </div>

        {/* The focused session's host owns the file drawer: a one-host SFTP
            panel, so its selector cannot drift away from the terminal above. */}
        {filesOpen && focusedSession && (
          <>
            <div
              role="separator"
              aria-orientation="horizontal"
              aria-label={t("Resize file drawer")}
              title={t("Resize file drawer")}
              className="group flex h-2 shrink-0 cursor-row-resize items-center justify-center border-t border-line bg-muted/60 transition-colors hover:bg-primary/40"
              onPointerDown={startFilesDrag}
            >
              <span className="h-0.5 w-8 rounded-full bg-foreground/25 group-hover:bg-primary-foreground/70" />
            </div>
            <div
              className="shrink-0 overflow-y-auto"
              style={{ height: `${filesRatio * 100}%` }}
            >
              {mountedDrawers.map((id) => {
                const session = sessions.find((candidate) => candidate.id === id);
                if (!session) return null;
                const drawerHosts = hosts.filter((host) => host.name === session.host);
                return (
                  <div
                    key={id}
                    className="h-full"
                    style={{ display: id === focusedSessionId ? "block" : "none" }}
                  >
                    {drawerHosts.length > 0 ? (
                      <SFTPPanel hosts={drawerHosts} initialHost={session.host} embedded />
                    ) : (
                      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
                        {t("Host no longer exists")}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </>
        )}
      </div>

      {broadcastOpen && (
        <Dialog onClose={() => !broadcastBusy && setBroadcastOpen(false)} className="max-w-2xl">
          <div className="grid gap-4">
            <div>
              <h2 className="text-lg font-semibold">{t("Broadcast command")}</h2>
              <p className="text-xs text-muted-foreground">
                {t("Review every target and its effective risk before sending. Delivery is authorization-atomic, not remote execution-atomic.")}
              </p>
            </div>
            <div className="grid max-h-36 grid-cols-2 gap-2 overflow-y-auto rounded border border-border p-2">
              {sessions
                .filter((session) => terminalConnections.has(session.id))
                .map((session) => (
                  <label
                    key={session.id}
                    className="flex items-center gap-2 rounded px-2 py-1 text-sm hover:bg-muted"
                  >
                    <input
                      type="checkbox"
                      checked={broadcastTargets.has(session.id)}
                      onChange={() => {
                        setBroadcastTargets((previous) => {
                          const next = new Set(previous);
                          if (next.has(session.id)) next.delete(session.id);
                          else next.add(session.id);
                          return next;
                        });
                        setBroadcastPreview(null);
                      }}
                    />
                    <span className="truncate">{labels.get(session.id) ?? session.host}</span>
                  </label>
                ))}
            </div>
            <Textarea
              autoFocus
              value={broadcastCommand}
              onChange={(event) => {
                setBroadcastCommand(event.target.value);
                setBroadcastPreview(null);
              }}
              placeholder={t("Single command to run on selected terminals")}
              className="min-h-20"
            />
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={broadcastForce}
                onChange={(event) => {
                  setBroadcastForce(event.target.checked);
                  setBroadcastPreview(null);
                }}
              />
              {t("Force high-risk commands when policy allows")}
            </label>
            {broadcastPreview && (
              <div className="grid gap-1 rounded border border-border p-2 text-xs">
                {broadcastPreview.targets.map((target) => (
                  <div key={target.terminal_id} className="flex items-center gap-2">
                    <span className="min-w-0 flex-1 truncate">{target.host}</span>
                    <RiskBadge level={target.risk_level} />
                    <span className={target.authorized ? "text-success" : "text-destructive"}>
                      {target.error
                        ? target.error
                        : target.requires_approval
                          ? t("Approval required")
                          : t("Ready")}
                    </span>
                  </div>
                ))}
              </div>
            )}
            {broadcastError && <div className="text-sm text-destructive">{broadcastError}</div>}
            <div className="flex justify-end gap-2">
              <Button
                variant="secondary"
                onClick={() => setBroadcastOpen(false)}
                disabled={broadcastBusy}
              >
                {t("Cancel")}
              </Button>
              <Button
                variant="secondary"
                onClick={() => void previewBroadcast()}
                disabled={broadcastBusy || broadcastTargets.size < 2 || !broadcastCommand.trim()}
              >
                {t("Preview risk")}
              </Button>
              <Button
                variant="destructive"
                onClick={() => void runBroadcast()}
                disabled={
                  broadcastBusy ||
                  !broadcastPreview ||
                  broadcastPreview.targets.some((target) => !target.authorized)
                }
              >
                {t("Broadcast and run on {count} terminals", { count: broadcastTargets.size })}
              </Button>
            </div>
          </div>
        </Dialog>
      )}
      <SnippetsDialog
        open={snippetsOpen}
        canInsert={Boolean(focusedSessionId)}
        onClose={() => setSnippetsOpen(false)}
        onInsert={insertSnippet}
      />
    </div>
  );
}

export default memo(TerminalPanel);