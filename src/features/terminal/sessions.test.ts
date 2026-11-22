import { beforeEach, describe, expect, it } from "vitest";
import {
  activateSession,
  assignPane,
  closeSession,
  emptyTerminalState,
  focusOrOpenSession,
  loadTerminalState,
  moveSession,
  openNewSession,
  paneCount,
  paneRect,
  pruneMissingHosts,
  saveTerminalState,
  sessionLabels,
  setFilesRatio,
  setLayout,
  setRatios,
  type TerminalState,
} from "./sessions";

const STORAGE_KEY = "agent2ssh.terminal.state";

function stateWith(hosts: string[], overrides: Partial<TerminalState> = {}): TerminalState {
  let state = emptyTerminalState();
  for (const host of hosts) state = openNewSession(state, host);
  return { ...state, ...overrides };
}

beforeEach(() => {
  localStorage.clear();
});

describe("pane geometry", () => {
  it("counts panes per layout and tiles them without gaps", () => {
    expect(paneCount("single")).toBe(1);
    expect(paneCount("row-2")).toBe(2);
    expect(paneCount("col-2")).toBe(2);
    expect(paneCount("grid-4")).toBe(4);

    const ratios = { col: 0.4, row: 0.6 };
    expect(paneRect("single", ratios, 0)).toEqual({ top: 0, left: 0, width: 100, height: 100 });
    // A horizontal split shares the full height and divides the width.
    expect(paneRect("row-2", ratios, 0)).toEqual({ top: 0, left: 0, width: 40, height: 100 });
    expect(paneRect("row-2", ratios, 1)).toEqual({ top: 0, left: 40, width: 60, height: 100 });
    // A vertical split shares the full width and divides the height.
    expect(paneRect("col-2", ratios, 0)).toEqual({ top: 0, left: 0, width: 100, height: 60 });
    expect(paneRect("col-2", ratios, 1)).toEqual({ top: 60, left: 0, width: 100, height: 40 });
    // The grid's four rects cover the area exactly once.
    const grid = [0, 1, 2, 3].map((index) => paneRect("grid-4", ratios, index));
    expect(grid.reduce((sum, rect) => sum + (rect.width * rect.height) / 100, 0)).toBeCloseTo(100);
    expect(grid[3]).toEqual({ top: 60, left: 40, width: 60, height: 40 });
  });
});

describe("opening sessions", () => {
  it("focuses the host's latest session on a tree click instead of piling up", () => {
    const first = focusOrOpenSession(emptyTerminalState(), "zos");
    expect(first.sessions.map((session) => session.host)).toEqual(["zos"]);
    expect(first.panes[first.focusedPane]).toBe(first.sessions[0].id);

    const again = focusOrOpenSession(first, "zos");
    expect(again.sessions).toBe(first.sessions); // same array — no duplicate session
    expect(again.panes).toEqual(first.panes);

    // An explicit second session to the same host is allowed, and a tree click
    // then focuses that newest one rather than the first.
    const second = openNewSession(again, "zos");
    expect(second.sessions).toHaveLength(2);
    expect(second.panes[second.focusedPane]).toBe(second.sessions[1].id);
    const clicked = focusOrOpenSession({ ...second, panes: [second.sessions[0].id] }, "zos");
    expect(clicked.panes[0]).toBe(second.sessions[1].id);
    expect(clicked.sessions).toHaveLength(2);
  });

  it("appends a session into the focused pane without disturbing the others", () => {
    const split = setLayout(stateWith(["a"]), "row-2");
    const next = openNewSession({ ...split, focusedPane: 1 }, "b");
    expect(next.sessions.map((session) => session.host)).toEqual(["a", "b"]);
    expect(next.panes[0]).toBe(split.panes[0]);
    expect(next.panes[1]).toBe(next.sessions[1].id);
    expect(next.focusedPane).toBe(1);
    expect(openNewSession(next, "")).toBe(next);
  });

  it("activates a session into the focused pane and ignores unknown ids", () => {
    const state = setLayout(stateWith(["a", "b"]), "row-2");
    const moved = activateSession({ ...state, focusedPane: 1 }, state.sessions[0].id);
    expect(moved.panes).toEqual([state.panes[0], state.sessions[0].id]);
    expect(moved.focusedPane).toBe(1);
    expect(activateSession(state, "nope")).toBe(state);
  });
  it("moves a session between panes instead of showing it in two at once", () => {
    const base = stateWith(["a", "b"]);
    const state = setLayout({ ...base, panes: [base.sessions[0].id, null] }, "row-2");
    // Assigning it to pane 2 frees pane 1: one terminal cannot render twice.
    const assigned = assignPane({ ...state, focusedPane: 0 }, 1, state.sessions[0].id);
    expect(assigned.panes).toEqual([null, state.sessions[0].id]);
    // The same holds for a tree click landing in the newly focused pane.
    const clicked = focusOrOpenSession(assigned, "a");
    expect(clicked.panes).toEqual([null, state.sessions[0].id]);
    expect(clicked.sessions).toHaveLength(2);
  });
});

describe("closing sessions", () => {
  it("frees every pane showing the session and refocuses a neighbour", () => {
    // Two panes both showing "b", plus a third session off screen.
    const base = stateWith(["a", "b", "c"]);
    const state = setLayout(
      { ...base, panes: [base.sessions[1].id, base.sessions[1].id, null, null] },
      "grid-4",
    );
    const focused = { ...state, focusedPane: 0 };
    const closed = closeSession(focused, focused.sessions[1].id);
    expect(closed.sessions.map((session) => session.host)).toEqual(["a", "c"]);
    // Both panes that showed "b" are freed; the focused one takes the right
    // neighbour ("c", which slid into index 1).
    expect(closed.panes).toEqual([closed.sessions[1].id, null, null, null]);
  });

  it("takes the left neighbour when the closed session was last", () => {
    const state = stateWith(["a", "b"]);
    const closed = closeSession({ ...state, focusedPane: 0 }, state.sessions[1].id);
    expect(closed.panes[0]).toBe(closed.sessions[0].id);
    // Closing the only session leaves an empty surface, not a dangling pane.
    const emptied = closeSession(closed, closed.sessions[0].id);
    expect(emptied.sessions).toEqual([]);
    expect(emptied.panes).toEqual([null]);
    expect(closeSession(state, "nope")).toBe(state);
  });

  it("leaves the focused pane alone when an off-screen session closes", () => {
    const state = setLayout(stateWith(["a", "b"]), "row-2");
    const assigned = assignPane(state, 1, state.sessions[1].id);
    const closed = closeSession({ ...assigned, focusedPane: 0 }, assigned.sessions[1].id);
    expect(closed.panes).toEqual([assigned.sessions[0].id, null]);
    expect(closed.focusedPane).toBe(0);
  });
});

describe("layout, ratios and order", () => {
  it("keeps surviving panes when the layout shrinks and pads when it grows", () => {
    const state = stateWith(["a", "b", "c", "d"]);
    const grid = setLayout(
      { ...state, panes: state.sessions.map((session) => session.id), focusedPane: 3 },
      "grid-4",
    );
    const single = setLayout(grid, "single");
    expect(single.panes).toEqual([grid.sessions[0].id]);
    expect(single.focusedPane).toBe(0);
    const back = setLayout(single, "grid-4");
    expect(back.panes).toEqual([grid.sessions[0].id, null, null, null]);
    expect(setLayout(grid, "row-2").panes).toHaveLength(2);
  });

  it("clamps split ratios to the range the drag handles enforce", () => {
    const state = emptyTerminalState();
    expect(setRatios(state, { col: 0.01, row: 0.99 }).ratios).toEqual({ col: 0.15, row: 0.85 });
    expect(setRatios(state, { col: 0.3, row: 0.7 }).ratios).toEqual({ col: 0.3, row: 0.7 });
  });

  it("clamps the drawer's height share to a usable band", () => {
    const state = stateWith(["a"]);
    expect(setFilesRatio(state, 0.01).filesRatio).toBe(0.25);
    expect(setFilesRatio(state, 0.99).filesRatio).toBe(0.75);
    expect(setFilesRatio(state, 0.6).filesRatio).toBe(0.6);
  });

  it("assigns only known sessions to real panes", () => {
    const state = setLayout(stateWith(["a"]), "row-2");
    expect(assignPane(state, 1, state.sessions[0].id).panes[1]).toBe(state.sessions[0].id);
    expect(assignPane(state, 1, null).panes[1]).toBeNull();
    expect(assignPane(state, 9, state.sessions[0].id)).toBe(state);
    expect(assignPane(state, 1, "nope")).toBe(state);
  });

  it("reorders the strip by dropping one session onto another's slot", () => {
    const state = stateWith(["a", "b", "c"]);
    const [a, , c] = state.sessions;
    expect(moveSession(state, a.id, c.id).sessions.map((session) => session.host)).toEqual([
      "b",
      "c",
      "a",
    ]);
    expect(moveSession(state, c.id, a.id).sessions.map((session) => session.host)).toEqual([
      "c",
      "a",
      "b",
    ]);
    expect(moveSession(state, a.id, a.id)).toBe(state);
    expect(moveSession(state, "nope", a.id)).toBe(state);
    // Reordering never changes which session a pane shows.
    expect(moveSession(state, a.id, c.id).panes).toEqual(state.panes);
  });

  it("labels repeated hosts with an ordinal so panes stay distinguishable", () => {
    const state = stateWith(["a", "b", "a"]);
    const labels = sessionLabels(state.sessions);
    expect(state.sessions.map((session) => labels.get(session.id))).toEqual(["a #1", "b", "a #2"]);
  });
});

describe("host removal", () => {
  it("prunes the missing host's sessions and refills the focused pane", () => {
    const state = setLayout(stateWith(["a", "gone", "c"]), "grid-4");
    const assigned = {
      ...state,
      panes: [state.sessions[1].id, state.sessions[2].id, null, null],
      focusedPane: 0,
    };
    const pruned = pruneMissingHosts(assigned, new Set(["a", "c"]));
    expect(pruned.sessions.map((session) => session.host)).toEqual(["a", "c"]);
    expect(pruned.panes).toEqual([pruned.sessions[0].id, pruned.sessions[1].id, null, null]);
    // Nothing to prune is a no-op, so the effect does not re-render on every poll.
    expect(pruneMissingHosts(pruned, new Set(["a", "c"]))).toBe(pruned);
    expect(pruneMissingHosts(pruned, new Set()).sessions).toEqual([]);
    expect(pruneMissingHosts(pruned, new Set()).panes).toEqual([null, null, null, null]);
  });
});

describe("persistence", () => {
  it("round-trips sessions, panes, layout, ratios and the drawer", () => {
    const state = setLayout(stateWith(["a", "b"]), "row-2");
    const saved = {
      ...state,
      panes: [state.sessions[1].id, state.sessions[0].id],
      focusedPane: 1,
      ratios: { col: 0.3, row: 0.7 },
      filesOpen: false,
      filesRatio: 0.62,
    };
    saveTerminalState(saved);
    expect(loadTerminalState()).toEqual(saved);
  });

  it("falls back to an empty surface for a missing or corrupt payload", () => {
    expect(loadTerminalState()).toEqual(emptyTerminalState());
    localStorage.setItem(STORAGE_KEY, "{not json");
    expect(loadTerminalState()).toEqual(emptyTerminalState());
    localStorage.setItem(STORAGE_KEY, "null");
    expect(loadTerminalState()).toEqual(emptyTerminalState());
    localStorage.setItem(STORAGE_KEY, "42");
    expect(loadTerminalState()).toEqual(emptyTerminalState());
  });

  it("drops pane slots that name a session the payload did not restore", () => {
    const state = stateWith(["a"]);
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        sessions: [...state.sessions, { id: 7, host: "bad" }, null],
        panes: ["ghost", state.sessions[0].id],
        layout: "row-2",
        focusedPane: 9,
        ratios: { col: 5, row: "wide" },
        filesOpen: "yes",
        filesRatio: 4,
      }),
    );
    const restored = loadTerminalState();
    expect(restored.sessions).toEqual(state.sessions);
    expect(restored.panes).toEqual([null, state.sessions[0].id]);
    expect(restored.focusedPane).toBe(1); // clamped into the restored layout
    expect(restored.ratios).toEqual({ col: 0.85, row: 0.5 });
    expect(restored.filesOpen).toBe(true);
    expect(restored.filesRatio).toBe(0.75);
    // An unknown layout falls back to single, which has exactly one pane.
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ ...state, layout: "hex-9" }));
    expect(loadTerminalState().layout).toBe("single");
    expect(loadTerminalState().panes).toEqual([state.sessions[0].id]);
  });
});