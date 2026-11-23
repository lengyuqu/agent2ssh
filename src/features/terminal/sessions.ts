/**
 * The terminal surface's session and pane state, as plain data plus pure
 * transitions. The Terminal module is the connection-first surface: a host
 * tree opens sessions, sessions land in panes, and the focused pane's host
 * owns the file drawer. Keeping the transitions here means the ordering rules
 * (which pane keeps focus after a close, what a restored layout may contain)
 * are testable without React or a daemon.
 */

export type TerminalSession = {
  id: string;
  /** Host profile name; resolved against the live host list on restore. */
  host: string;
  createdAt: number;
};

export type PaneLayout = "single" | "row-2" | "col-2" | "grid-4";

export type Rect = { top: number; left: number; width: number; height: number };

export type TerminalState = {
  sessions: TerminalSession[];
  /** Session id per pane slot, `null` for an empty pane. Length = paneCount(layout). */
  panes: Array<string | null>;
  layout: PaneLayout;
  focusedPane: number;
  /** Split ratios, clamped to the same range the drag handles enforce. */
  ratios: { col: number; row: number };
  filesOpen: boolean;
  /** Share of the stage height given to the file drawer. */
  filesRatio: number;
};

const STORAGE_KEY = "agent2ssh.terminal.state";
const LAYOUTS: PaneLayout[] = ["single", "row-2", "col-2", "grid-4"];

export function paneCount(layout: PaneLayout): number {
  if (layout === "single") return 1;
  if (layout === "grid-4") return 4;
  return 2;
}

export function paneRect(
  layout: PaneLayout,
  ratios: { col: number; row: number },
  index: number,
): Rect {
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
  const rects: Rect[] = [
    { top: 0, left: 0, width: colPct, height: rowPct },
    { top: 0, left: colPct, width: 100 - colPct, height: rowPct },
    { top: rowPct, left: 0, width: colPct, height: 100 - rowPct },
    { top: rowPct, left: colPct, width: 100 - colPct, height: 100 - rowPct },
  ];
  return rects[index];
}

export function emptyTerminalState(): TerminalState {
  return {
    sessions: [],
    panes: [null],
    layout: "single",
    focusedPane: 0,
    ratios: { col: 0.5, row: 0.5 },
    filesOpen: true,
    filesRatio: 0.45,
  };
}

let seq = 0;

function nextSessionId(): string {
  seq += 1;
  return `term-${seq}-${Date.now()}`;
}

/**
 * Put a session in the focused pane, leaving the session list untouched. A
 * session renders into one pane only, so any other pane showing it is freed —
 * otherwise that pane's header would name a terminal it cannot display.
 */
function withPane(state: TerminalState, paneIndex: number, sessionId: string | null): TerminalState {
  const panes = state.panes.map((id, index) => {
    if (index === paneIndex) return sessionId;
    return sessionId !== null && id === sessionId ? null : id;
  });
  return { ...state, panes, focusedPane: paneIndex };
}

/** Put a session in the focused pane, leaving the session list untouched. */
export function activateSession(state: TerminalState, sessionId: string): TerminalState {
  if (!state.sessions.some((session) => session.id === sessionId)) return state;
  return withPane(state, state.focusedPane, sessionId);
}

/**
 * A host-tree click, and the drawer's "open in terminal" handoff: focus the
 * host's most recent session when it has one, otherwise open a new one.
 * Repeat clicks therefore never pile up sessions. `withPane` keeps that
 * session in the focused pane only — one session, one pane.
 */
export function focusOrOpenSession(state: TerminalState, host: string): TerminalState {
  const existing = [...state.sessions].reverse().find((session) => session.host === host);
  if (existing) return activateSession(state, existing.id);
  return openNewSession(state, host);
}

/** An explicit new session, even when the host already has one. */
export function openNewSession(state: TerminalState, host: string): TerminalState {
  if (!host) return state;
  const session: TerminalSession = { id: nextSessionId(), host, createdAt: Date.now() };
  return {
    ...withPane(state, state.focusedPane, session.id),
    sessions: [...state.sessions, session],
  };
}

/**
 * Close a session and free every pane showing it. When the focused pane goes
 * empty it takes the closed session's neighbour — right first, else left — so
 * closing never leaves the operator staring at an empty pane while sessions
 * are still open.
 */
export function closeSession(state: TerminalState, sessionId: string): TerminalState {
  const index = state.sessions.findIndex((session) => session.id === sessionId);
  if (index === -1) return state;
  const sessions = state.sessions.filter((session) => session.id !== sessionId);
  let panes = state.panes.map((id) => (id === sessionId ? null : id));
  if (panes[state.focusedPane] === null && sessions.length > 0) {
    const neighbour = sessions[Math.min(index, sessions.length - 1)];
    panes = panes.map((id, paneIndex) => (paneIndex === state.focusedPane ? neighbour.id : id));
  }
  return { ...state, sessions, panes };
}

export function assignPane(
  state: TerminalState,
  paneIndex: number,
  sessionId: string | null,
): TerminalState {
  if (paneIndex < 0 || paneIndex >= state.panes.length) return state;
  if (sessionId !== null && !state.sessions.some((session) => session.id === sessionId)) return state;
  return withPane(state, paneIndex, sessionId);
}

/** Switch layout, keeping the sessions already on screen in the surviving panes. */
export function setLayout(state: TerminalState, layout: PaneLayout): TerminalState {
  const count = paneCount(layout);
  const panes = state.panes.slice(0, count);
  while (panes.length < count) panes.push(null);
  return { ...state, layout, panes, focusedPane: Math.min(state.focusedPane, count - 1) };
}

export function setRatios(
  state: TerminalState,
  ratios: { col: number; row: number },
): TerminalState {
  return {
    ...state,
    ratios: {
      col: Math.min(0.85, Math.max(0.15, ratios.col)),
      row: Math.min(0.85, Math.max(0.15, ratios.row)),
    },
  };
}

/**
 * Height share of the file drawer. The floor keeps its toolbar and a few file
 * rows on screen; the ceiling keeps a usable terminal above it.
 */
export function setFilesRatio(state: TerminalState, ratio: number): TerminalState {
  return { ...state, filesRatio: Math.min(0.75, Math.max(0.25, ratio)) };
}

/** Reorder the session strip by dropping `dragId` onto `overId`'s slot. */
export function moveSession(
  state: TerminalState,
  dragId: string,
  overId: string,
): TerminalState {
  if (dragId === overId) return state;
  const from = state.sessions.findIndex((session) => session.id === dragId);
  const over = state.sessions.findIndex((session) => session.id === overId);
  if (from === -1 || over === -1) return state;
  const sessions = [...state.sessions];
  const [moved] = sessions.splice(from, 1);
  sessions.splice(over, 0, moved);
  return { ...state, sessions };
}

/** A host removed from the configuration takes its sessions and panes with it. */
export function pruneMissingHosts(
  state: TerminalState,
  hostNames: ReadonlySet<string>,
): TerminalState {
  const sessions = state.sessions.filter((session) => hostNames.has(session.host));
  if (sessions.length === state.sessions.length) return state;
  const kept = new Set(sessions.map((session) => session.id));
  let panes = state.panes.map((id) => (id !== null && kept.has(id) ? id : null));
  if (panes[state.focusedPane] === null && sessions.length > 0) {
    panes = panes.map((id, index) => (index === state.focusedPane ? sessions[0].id : id));
  }
  return { ...state, sessions, panes };
}

/** Host name, plus an ordinal when one host has several sessions open. */
export function sessionLabels(sessions: TerminalSession[]): Map<string, string> {
  const totals = new Map<string, number>();
  for (const session of sessions) totals.set(session.host, (totals.get(session.host) ?? 0) + 1);
  const seen = new Map<string, number>();
  const labels = new Map<string, string>();
  for (const session of sessions) {
    const ordinal = (seen.get(session.host) ?? 0) + 1;
    seen.set(session.host, ordinal);
    labels.set(
      session.id,
      (totals.get(session.host) ?? 0) > 1 ? `${session.host} #${ordinal}` : session.host,
    );
  }
  return labels;
}

export function saveTerminalState(state: TerminalState): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // Opaque-origin / quota failures must never break the session itself.
  }
}

function isLayout(value: unknown): value is PaneLayout {
  return typeof value === "string" && LAYOUTS.includes(value as PaneLayout);
}

function clampRatio(value: unknown, fallback: number, min = 0.15, max = 0.85): number {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.min(max, Math.max(min, value))
    : fallback;
}

/**
 * Restore the last surface. Sessions are kept only when their id and host are
 * strings; pane slots are kept only when they name a restored session, so a
 * stale or hand-edited payload cannot mount a terminal for a session that no
 * longer exists.
 */
export function loadTerminalState(): TerminalState {
  const empty = emptyTerminalState();
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return empty;
    const parsed = JSON.parse(raw) as Partial<TerminalState> | null;
    if (typeof parsed !== "object" || parsed === null) return empty;
    const sessions = Array.isArray(parsed.sessions)
      ? parsed.sessions.filter(
          (session): session is TerminalSession =>
            typeof session === "object" &&
            session !== null &&
            typeof (session as TerminalSession).id === "string" &&
            typeof (session as TerminalSession).host === "string",
        )
      : [];
    const layout = isLayout(parsed.layout) ? parsed.layout : empty.layout;
    const count = paneCount(layout);
    const ids = new Set(sessions.map((session) => session.id));
    const rawPanes = Array.isArray(parsed.panes) ? parsed.panes : [];
    const panes = Array.from({ length: count }, (_, index) => {
      const id = rawPanes[index];
      return typeof id === "string" && ids.has(id) ? id : null;
    });
    const focusedPane =
      typeof parsed.focusedPane === "number" && Number.isInteger(parsed.focusedPane)
        ? Math.min(Math.max(parsed.focusedPane, 0), count - 1)
        : 0;
    const ratios = {
      col: clampRatio(parsed.ratios?.col, empty.ratios.col),
      row: clampRatio(parsed.ratios?.row, empty.ratios.row),
    };
    return {
      sessions,
      panes,
      layout,
      focusedPane,
      ratios,
      filesOpen: typeof parsed.filesOpen === "boolean" ? parsed.filesOpen : empty.filesOpen,
      filesRatio: clampRatio(parsed.filesRatio, empty.filesRatio, 0.25, 0.75),
    };
  } catch {
    return empty;
  }
}