import type { ModuleId } from "../../nav";

/**
 * One workspace session: a host connection presented as a paired
 * terminal + file-drawer workspace, one tab each. Plain data only — the
 * open/close/activate transitions live in the pure helpers below so they can
 * be tested without React.
 */
export type WorkspaceTab = {
  id: string;
  /** The host profile name; resolved against the live host list on restore. */
  host: string;
  createdAt: number;
};

const STORAGE_KEY = "agent2ssh.workspace.tabs";

/** Open (or focus) the tab for a host. Returns the next tab list + active id. */
export function openHostTab(
  tabs: WorkspaceTab[],
  activeTabId: string | null,
  host: string,
): { tabs: WorkspaceTab[]; activeTabId: string } {
  const existing = tabs.find((tab) => tab.host === host);
  if (existing) {
    return { tabs, activeTabId: existing.id };
  }
  const tab: WorkspaceTab = {
    id: `ws-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    host,
    createdAt: Date.now(),
  };
  return { tabs: [...tabs, tab], activeTabId: tab.id };
}

/** Close a tab; the active tab moves to the right neighbor, else the left. */
export function closeTab(
  tabs: WorkspaceTab[],
  activeTabId: string | null,
  tabId: string,
): { tabs: WorkspaceTab[]; activeTabId: string | null } {
  const index = tabs.findIndex((tab) => tab.id === tabId);
  if (index === -1) {
    return { tabs, activeTabId };
  }
  const next = tabs.filter((tab) => tab.id !== tabId);
  let nextActive = activeTabId === tabId ? null : activeTabId;
  if (nextActive === null && next.length > 0) {
    const neighbor = next[Math.min(index, next.length - 1)];
    nextActive = neighbor.id;
  }
  return { tabs: next, activeTabId: nextActive };
}

/** Drop tabs whose host no longer exists (called after the host list loads). */
export function pruneMissingHosts(tabs: WorkspaceTab[], hostNames: Set<string>): WorkspaceTab[] {
  return tabs.filter((tab) => hostNames.has(tab.host));
}

/** Reorder tabs by dropping `dragId` onto `overId`'s slot. No-op if either id
 *  is unknown or they are equal. */
export function moveTab(tabs: WorkspaceTab[], dragId: string, overId: string): WorkspaceTab[] {
  if (dragId === overId) return tabs;
  const from = tabs.findIndex((tab) => tab.id === dragId);
  const over = tabs.findIndex((tab) => tab.id === overId);
  if (from === -1 || over === -1) return tabs;
  const next = [...tabs];
  const [moved] = next.splice(from, 1);
  next.splice(over, 0, moved);
  return next;
}

export function saveWorkspaceState(tabs: WorkspaceTab[], activeTabId: string | null): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ tabs, activeTabId }));
  } catch {
    // Opaque-origin / quota failures must never break the session itself.
  }
}

export function loadWorkspaceState(): { tabs: WorkspaceTab[]; activeTabId: string | null } {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { tabs: [], activeTabId: null };
    const parsed = JSON.parse(raw) as { tabs?: unknown; activeTabId?: unknown };
    if (!Array.isArray(parsed.tabs)) return { tabs: [], activeTabId: null };
    const tabs = parsed.tabs.filter(
      (tab): tab is WorkspaceTab =>
        typeof tab === "object" &&
        tab !== null &&
        typeof (tab as WorkspaceTab).id === "string" &&
        typeof (tab as WorkspaceTab).host === "string",
    );
    const activeTabId =
      typeof parsed.activeTabId === "string" && tabs.some((tab) => tab.id === parsed.activeTabId)
        ? parsed.activeTabId
        : (tabs[0]?.id ?? null);
    return { tabs, activeTabId };
  } catch {
    return { tabs: [], activeTabId: null };
  }
}

/** The management surface the workspace can navigate to (module pages). */
export type ManagementTarget = ModuleId;
