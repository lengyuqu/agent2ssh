import {
  ChevronDown,
  ChevronRight,
  Columns2,
  History,
  LayoutGrid,
  Plus,
  Rows2,
  Square,
  TerminalSquare,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../i18n";
import type { HostProfile } from "../types";
import {
  isTerminalThemeId,
  TERMINAL_THEME_OPTIONS,
  TERMINAL_THEME_STORAGE_KEY,
  terminalThemeBackground,
  type TerminalThemeId,
} from "../terminalThemes";
import { useTheme, type Theme as AppTheme } from "../theme";
import TerminalView, { type TerminalViewHandle } from "./TerminalView";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { IconButton } from "./ui/icon-button";
import { Input } from "./ui/input";
import { Select } from "./ui/select";
import { cn } from "../lib/utils";

type Props = {
  hosts: HostProfile[];
  initialHost?: string;
};

type Tab = { id: string; host: string };

// V3-2: up to 4 panes. "single" is the pre-V3-2 behavior (one tab at a time).
type Layout = "single" | "row-2" | "col-2" | "grid-4";

const PANE_HEADER_PX = 28;
const HISTORY_MAX_ENTRIES = 200;

type Rect = { top: number; left: number; width: number; height: number };

function paneCount(layout: Layout): number {
  if (layout === "single") return 1;
  if (layout === "grid-4") return 4;
  return 2;
}

function paneRect(layout: Layout, ratios: { col: number; row: number }, index: number): Rect {
  const colPct = ratios.col * 100;
  const rowPct = ratios.row * 100;
  if (layout === "single") return { top: 0, left: 0, width: 100, height: 100 };
  if (layout === "row-2") {
    return index === 0
      ? { top: 0, left: 0, width: colPct, height: 100 }
      : { top: 0, left: colPct, width: 100 - colPct, height: 100 };
  }
  if (layout === "col-2") {
    return index === 0
      ? { top: 0, left: 0, width: 100, height: rowPct }
      : { top: rowPct, left: 0, width: 100, height: 100 - rowPct };
  }
  // grid-4
  const rects: Rect[] = [
    { top: 0, left: 0, width: colPct, height: rowPct },
    { top: 0, left: colPct, width: 100 - colPct, height: rowPct },
    { top: rowPct, left: 0, width: colPct, height: 100 - rowPct },
    { top: rowPct, left: colPct, width: 100 - colPct, height: 100 - rowPct },
  ];
  return rects[index];
}

type WindowWithTerminalSeq = Window & {
  __agent2sshTerminalSeq?: number;
};

function nextId(): string {
  if (typeof window === "undefined") {
    return `term-${Date.now()}`;
  }
  const next = ((window as WindowWithTerminalSeq).__agent2sshTerminalSeq ?? 0) + 1;
  (window as WindowWithTerminalSeq).__agent2sshTerminalSeq = next;
  return `term-${next}-${Date.now()}`;
}

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

export default function TerminalPanel({ hosts, initialHost = "" }: Props) {
  const { t } = useI18n();
  const { theme: appTheme } = useTheme();
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [newHost, setNewHost] = useState(initialHost || hosts[0]?.name || "");
  const [terminalTheme, setTerminalTheme] = useState<TerminalThemeId>(() => initialTerminalTheme());
  const [resolvedSystemTheme, setResolvedSystemTheme] = useState<AppTheme>(() => systemTheme());

  // V3-2: split-screen layout state.
  const [layout, setLayout] = useState<Layout>("single");
  const [paneTabIds, setPaneTabIds] = useState<Array<string | null>>([null]);
  const [focusedPane, setFocusedPane] = useState(0);
  const [ratios, setRatios] = useState({ col: 0.5, row: 0.5 });
  const containerRef = useRef<HTMLDivElement>(null);

  // V3-2: per-tab typed-line history (Ctrl+R search) and the TerminalView
  // handles used to inject a picked entry back into the right pane.
  const historyRef = useRef<Map<string, string[]>>(new Map());
  const terminalRefs = useRef<Map<string, TerminalViewHandle>>(new Map());
  const [historySearch, setHistorySearch] = useState<{ paneIndex: number; query: string } | null>(
    null
  );
  const [collapsedHosts, setCollapsedHosts] = useState<Set<string>>(() => new Set());

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
    [effectiveAppTheme, terminalTheme]
  );

  function changeLayout(next: Layout) {
    setLayout(next);
    const count = paneCount(next);
    setPaneTabIds((prev) => {
      const resized = prev.slice(0, count);
      while (resized.length < count) resized.push(null);
      return resized;
    });
    setFocusedPane((prev) => Math.min(prev, count - 1));
  }

  function assignPane(paneIndex: number, tabId: string | null) {
    setPaneTabIds((prev) => prev.map((id, i) => (i === paneIndex ? tabId : id)));
    setFocusedPane(paneIndex);
  }

  function openTab(host: string) {
    if (!host) return;
    const id = nextId();
    setTabs((prev) => [...prev, { id, host }]);
    assignPane(focusedPane, id);
  }

  function closeTab(id: string) {
    setTabs((prev) => prev.filter((tab) => tab.id !== id));
    setPaneTabIds((prev) => prev.map((tabId) => (tabId === id ? null : tabId)));
    historyRef.current.delete(id);
    terminalRefs.current.delete(id);
  }

  function recordLine(tabId: string, line: string) {
    const list = historyRef.current.get(tabId) ?? [];
    const deduped = list.filter((entry) => entry !== line);
    deduped.unshift(line);
    historyRef.current.set(tabId, deduped.slice(0, HISTORY_MAX_ENTRIES));
  }

  function pickHistoryEntry(paneIndex: number, tabId: string, text: string) {
    terminalRefs.current.get(tabId)?.sendText(text);
    setHistorySearch(null);
    terminalRefs.current.get(tabId)?.focus();
    setFocusedPane(paneIndex);
  }

  function toggleHostGroup(host: string) {
    setCollapsedHosts((prev) => {
      const next = new Set(prev);
      if (next.has(host)) next.delete(host);
      else next.add(host);
      return next;
    });
  }

  function startDrag(axis: "col" | "row", event: React.PointerEvent) {
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
      setRatios((prev) => ({ ...prev, [axis]: clamped }));
    }
    function onUp() {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    }
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }

  const tabsByHost = useMemo(() => {
    const map = new Map<string, Tab[]>();
    for (const tab of tabs) {
      const list = map.get(tab.host) ?? [];
      list.push(tab);
      map.set(tab.host, list);
    }
    return map;
  }, [tabs]);

  const visiblePanes = Array.from({ length: paneCount(layout) }, (_, i) => i);
  const searchTabId = historySearch ? paneTabIds[historySearch.paneIndex] : null;
  const searchResults = useMemo(() => {
    if (!historySearch || !searchTabId) return [];
    const all = historyRef.current.get(searchTabId) ?? [];
    const needle = historySearch.query.trim().toLowerCase();
    return needle ? all.filter((line) => line.toLowerCase().includes(needle)) : all;
  }, [historySearch, searchTabId]);

  return (
    <Card className="flex h-[72vh] overflow-hidden p-0">
      {/* V3-2: session tree grouped by host, plus layout controls. */}
      <div className="flex w-[220px] shrink-0 flex-col gap-3 overflow-y-auto border-r border-border bg-muted/30 p-2.5">
        <div className="grid gap-1.5">
          <div className="px-1 text-[11px] font-bold uppercase tracking-wide text-muted-foreground">
            {t("Split view")}
          </div>
          <div className="grid grid-cols-4 gap-1">
            <IconButton
              size="sm"
              variant={layout === "single" ? "default" : "ghost"}
              title={t("Single pane")}
              onClick={() => changeLayout("single")}
              className={layout === "single" ? "border-primary/40 bg-primary/10 text-primary" : ""}
            >
              <Square size={14} />
            </IconButton>
            <IconButton
              size="sm"
              variant={layout === "row-2" ? "default" : "ghost"}
              title={t("Split horizontally")}
              onClick={() => changeLayout("row-2")}
              className={layout === "row-2" ? "border-primary/40 bg-primary/10 text-primary" : ""}
            >
              <Columns2 size={14} />
            </IconButton>
            <IconButton
              size="sm"
              variant={layout === "col-2" ? "default" : "ghost"}
              title={t("Split vertically")}
              onClick={() => changeLayout("col-2")}
              className={layout === "col-2" ? "border-primary/40 bg-primary/10 text-primary" : ""}
            >
              <Rows2 size={14} />
            </IconButton>
            <IconButton
              size="sm"
              variant={layout === "grid-4" ? "default" : "ghost"}
              title={t("Split into 4")}
              onClick={() => changeLayout("grid-4")}
              className={layout === "grid-4" ? "border-primary/40 bg-primary/10 text-primary" : ""}
            >
              <LayoutGrid size={14} />
            </IconButton>
          </div>
        </div>

        <div className="grid gap-1.5">
          <div className="px-1 text-[11px] font-bold uppercase tracking-wide text-muted-foreground">
            {t("New terminal")}
          </div>
          <Select
            value={newHost}
            onChange={(e) => setNewHost(e.target.value)}
            className="h-8 text-xs"
          >
            {hosts.length === 0 && <option value="">{t("No hosts")}</option>}
            {hosts.map((host) => (
              <option key={host.name} value={host.name}>
                {host.name}
              </option>
            ))}
          </Select>
          <Button size="sm" onClick={() => openTab(newHost)} disabled={!newHost} className="h-8">
            <Plus size={13} />
            {t("New terminal")}
          </Button>
        </div>

        <div className="grid min-h-0 flex-1 gap-1 overflow-y-auto">
          <div className="px-1 text-[11px] font-bold uppercase tracking-wide text-muted-foreground">
            {t("Sessions")}
          </div>
          {tabs.length === 0 && (
            <div className="px-1 text-xs text-muted-foreground">{t("No active sessions")}</div>
          )}
          {[...tabsByHost.entries()].map(([host, hostTabs]) => {
            const collapsed = collapsedHosts.has(host);
            return (
              <div key={host} className="grid gap-0.5">
                <button
                  type="button"
                  onClick={() => toggleHostGroup(host)}
                  className="flex items-center gap-1 rounded px-1 py-1 text-left text-xs font-semibold text-foreground/85 hover:bg-muted"
                >
                  {collapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
                  <span className="truncate">{host}</span>
                  <span className="ml-auto text-[10px] font-normal text-muted-foreground">
                    {hostTabs.length}
                  </span>
                </button>
                {!collapsed &&
                  hostTabs.map((tab) => {
                    const paneIndex = paneTabIds.indexOf(tab.id);
                    return (
                      <div
                        key={tab.id}
                        onClick={() => assignPane(focusedPane, tab.id)}
                        className={cn(
                          "group flex cursor-pointer items-center gap-1.5 rounded px-2 py-1 pl-5 text-xs transition-colors hover:bg-muted",
                          paneIndex >= 0 ? "text-primary" : "text-muted-foreground"
                        )}
                      >
                        <TerminalSquare size={11} className="shrink-0" />
                        <span className="flex-1 truncate">{tab.id.split("-")[1] ?? tab.id}</span>
                        {paneIndex >= 0 && (
                          <span className="rounded bg-primary/15 px-1 text-[10px] font-bold text-primary">
                            {paneIndex + 1}
                          </span>
                        )}
                        <span
                          role="button"
                          tabIndex={-1}
                          onClick={(e) => {
                            e.stopPropagation();
                            closeTab(tab.id);
                          }}
                          className="rounded p-0.5 opacity-0 hover:bg-foreground/10 group-hover:opacity-100"
                          aria-label={t("Close")}
                        >
                          <X size={11} />
                        </span>
                      </div>
                    );
                  })}
              </div>
            );
          })}
        </div>

        <label className="grid gap-1 text-xs font-medium text-muted-foreground">
          <span>{t("Terminal theme")}</span>
          <Select
            value={terminalTheme}
            onChange={(e) => {
              const next = e.target.value;
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
      </div>

      {/* Pane grid */}
      <div ref={containerRef} className="relative min-w-0 flex-1" style={{ backgroundColor: terminalBackground }}>
        {tabs.length === 0 && (
          <div className="absolute inset-0 flex items-center justify-center text-sm text-white/60">
            {t("Open a terminal to a host to get started.")}
          </div>
        )}

        {/* Pane chrome: header + border + focus ring, one per visible pane slot. */}
        {visiblePanes.map((paneIndex) => {
          const rect = paneRect(layout, ratios, paneIndex);
          const tabId = paneTabIds[paneIndex];
          const tab = tabs.find((candidate) => candidate.id === tabId);
          return (
            <div
              key={`chrome-${paneIndex}`}
              className={cn(
                "pointer-events-none absolute z-[2] border",
                paneIndex === focusedPane ? "border-primary/60" : "border-white/10"
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
                onClick={() => setFocusedPane(paneIndex)}
              >
                <TerminalSquare size={11} className="shrink-0" />
                <Select
                  value={tabId ?? ""}
                  onChange={(e) => assignPane(paneIndex, e.target.value || null)}
                  className="h-6 max-w-[140px] border-white/15 bg-transparent px-1 text-[11px] text-white/80"
                >
                  <option value="">{t("Select a session")}</option>
                  {tabs.map((candidate) => (
                    <option key={candidate.id} value={candidate.id}>
                      {candidate.host}
                    </option>
                  ))}
                </Select>
                {paneCount(layout) > 1 && (
                  <IconButton
                    size="sm"
                    title={t("Search command history")}
                    onClick={() => setHistorySearch({ paneIndex, query: "" })}
                    disabled={!tabId}
                    className="ml-auto h-6 w-6 border-white/15 bg-transparent text-white/70 hover:text-white"
                  >
                    <History size={12} />
                  </IconButton>
                )}
              </div>
              {!tab && (
                <div className="pointer-events-none flex h-[calc(100%-28px)] items-center justify-center text-xs text-white/40">
                  {t("Pick a session above")}
                </div>
              )}
            </div>
          );
        })}

        {/* Resize handles. */}
        {(layout === "row-2" || layout === "grid-4") && (
          <div
            className="absolute inset-y-0 z-10 w-1.5 -translate-x-1/2 cursor-col-resize hover:bg-primary/40"
            style={{ left: `${ratios.col * 100}%` }}
            onPointerDown={(e) => startDrag("col", e)}
          />
        )}
        {(layout === "col-2" || layout === "grid-4") && (
          <div
            className="absolute inset-x-0 z-10 h-1.5 -translate-y-1/2 cursor-row-resize hover:bg-primary/40"
            style={{ top: `${ratios.row * 100}%` }}
            onPointerDown={(e) => startDrag("row", e)}
          />
        )}

        {/* Live terminals: every open tab stays mounted (background sessions keep
            running) but only positioned into view when assigned to a pane. */}
        {tabs.map((tab) => {
          const paneIndex = paneTabIds.indexOf(tab.id);
          const assigned = paneIndex >= 0;
          const rect = assigned ? paneRect(layout, ratios, paneIndex) : null;
          return (
            <div
              key={tab.id}
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
              <TerminalView
                ref={(handle) => {
                  if (handle) terminalRefs.current.set(tab.id, handle);
                  else terminalRefs.current.delete(tab.id);
                }}
                host={tab.host}
                terminalTheme={terminalTheme}
                appTheme={effectiveAppTheme}
                onLineTyped={(line) => recordLine(tab.id, line)}
                onHistoryRequest={
                  assigned ? () => setHistorySearch({ paneIndex, query: "" }) : undefined
                }
              />
            </div>
          );
        })}

        {historySearch && searchTabId && (
          <div
            className="absolute inset-x-0 top-2 z-20 mx-auto w-[min(420px,90%)] rounded-lg border border-border bg-popover text-popover-foreground shadow-2xl"
            style={{
              left: `${paneRect(layout, ratios, historySearch.paneIndex).left}%`,
              width: `min(420px, ${paneRect(layout, ratios, historySearch.paneIndex).width}% - 16px)`,
            }}
          >
            <div className="flex items-center gap-2 border-b border-border px-3 py-2">
              <History size={14} className="shrink-0 text-muted-foreground" />
              <Input
                autoFocus
                value={historySearch.query}
                onChange={(e) => setHistorySearch({ ...historySearch, query: e.target.value })}
                onKeyDown={(e) => {
                  if (e.key === "Escape") {
                    e.preventDefault();
                    setHistorySearch(null);
                  } else if (e.key === "Enter" && searchResults[0]) {
                    e.preventDefault();
                    pickHistoryEntry(historySearch.paneIndex, searchTabId, searchResults[0]);
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
                  onClick={() => pickHistoryEntry(historySearch.paneIndex, searchTabId, line)}
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
    </Card>
  );
}
