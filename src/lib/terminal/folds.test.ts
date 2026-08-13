import { describe, it, expect } from "vitest";
import { createFoldStore } from "./folds";
import type { CommandBlock, CommandBlockTracker } from "./command-blocks";

/* ─────────────────────────────────────────────────────────────
 * Fake xterm.js Terminal —— 复刻 folds.ts 用到的全部私有/公有 API。
 * 关键忠实点：
 *   - buf.lines.splice 触发 marker 自动迁移（行号修正 + 范围内 dispose）
 *   - 同时维护 ybase/ydisp/y 的语义不变量
 * ───────────────────────────────────────────────────────────── */

interface FakeMarker {
  id: number;
  line: number;
  isDisposed: boolean;
  onDispose(fn: () => void): { dispose: () => void };
  dispose(): void;
}

interface FakeLine {
  content: string;
  isWrapped?: boolean;
  getTrimmedLength?: () => number;
}

function fakeBlankLine(): FakeLine {
  const line: FakeLine = { content: "<blank>", isWrapped: false };
  line.getTrimmedLength = () => line.content === "<blank>" ? 0 : line.content.length;
  return line;
}

function fakeTerm(opts: { rows: number; initialLines: number; cursorY: number; ybase?: number; maxLength?: number }) {
  const rows = opts.rows;
  const maxLength = opts.maxLength;
  let ybase = opts.ybase ?? 0;
  let ydisp = ybase;
  let y = opts.cursorY;
  const lineArray: FakeLine[] = [];
  for (let i = 0; i < opts.initialLines; i++) lineArray.push({ content: `L${i}` });

  let markerSeq = 0;
  const markers: FakeMarker[] = [];

  const makeMarker = (line: number): FakeMarker => {
    const onDisposeFns: Array<() => void> = [];
    const m: FakeMarker = {
      id: ++markerSeq,
      line,
      isDisposed: false,
      onDispose(fn) {
        onDisposeFns.push(fn);
        return { dispose: () => {} };
      },
      dispose() {
        if (this.isDisposed) return;
        this.isDisposed = true;
        this.line = -1;
        for (const f of onDisposeFns) f();
      },
    };
    markers.push(m);
    return m;
  };

  function trimHead(count: number) {
    if (count <= 0) return;
    lineArray.splice(0, count);
    for (const m of markers) {
      if (m.isDisposed) continue;
      m.line -= count;
      if (m.line < 0) m.dispose();
    }
  }

  function enforceMaxLength() {
    if (!maxLength || lineArray.length <= maxLength) return;
    trimHead(lineArray.length - maxLength);
  }

  const lines = {
    get length() {
      return lineArray.length;
    },
    get(i: number) {
      return lineArray[i];
    },
    splice(start: number, deleteCount: number, ...items: FakeLine[]) {
      if (deleteCount > 0) {
        lineArray.splice(start, deleteCount);
        for (const m of markers) {
          if (m.isDisposed) continue;
          if (m.line >= start && m.line < start + deleteCount) m.dispose();
          else if (m.line >= start + deleteCount) m.line -= deleteCount;
        }
      }
      if (items.length > 0) {
        lineArray.splice(start, 0, ...items);
        for (const m of markers) {
          if (m.isDisposed) continue;
          if (m.line >= start) m.line += items.length;
        }
        enforceMaxLength();
      }
    },
    push(item: FakeLine) {
      lineArray.push(item);
    },
  };

  const buffer = {
    lines,
    get ybase() { return ybase; },
    set ybase(v: number) { ybase = v; },
    get ydisp() { return ydisp; },
    set ydisp(v: number) { ydisp = v; },
    get y() { return y; },
    set y(v: number) { y = v; },
    getBlankLine: (_attr: unknown) => fakeBlankLine(),
    addMarker: (line: number) => makeMarker(line),
  };

  const cursorMoveListeners = new Set<() => void>();
  const lineFeedListeners = new Set<() => void>();
  const writeParsedListeners = new Set<() => void>();
  let activeBufferType: "normal" | "alternate" = "normal";

  const term = {
    rows,
    buffer: {
      get active() {
        return { type: activeBufferType };
      },
    },
    refresh: (_a: number, _b: number) => {},
    onCursorMove(fn: () => void) {
      cursorMoveListeners.add(fn);
      return { dispose: () => cursorMoveListeners.delete(fn) };
    },
    onLineFeed(fn: () => void) {
      lineFeedListeners.add(fn);
      return { dispose: () => lineFeedListeners.delete(fn) };
    },
    onWriteParsed(fn: () => void) {
      writeParsedListeners.add(fn);
      return { dispose: () => writeParsedListeners.delete(fn) };
    },
    _core: { buffer },
  };

  return {
    term: term as unknown as Parameters<typeof createFoldStore>[0],
    buffer,
    lineContents: () => lineArray.map((l) => l.content),
    lineRefs: () => [...lineArray],
    makeMarker,
    lineFeed: () => {
      y = Math.min(rows - 1, y + 1);
      for (const fn of lineFeedListeners) fn();
    },
    moveCursorTo: (nextY: number) => {
      y = nextY;
      for (const fn of cursorMoveListeners) fn();
    },
    fireCursorMove: () => {
      for (const fn of cursorMoveListeners) fn();
    },
    fireWriteParsed: () => {
      for (const fn of writeParsedListeners) fn();
    },
    setActiveBuffer: (type: "normal" | "alternate") => {
      activeBufferType = type;
    },
    snapshot: () => ({
      length: lineArray.length,
      ybase,
      ydisp,
      y,
      cursorAbs: ybase + y,
    }),
  };
}

function fakeTracker(blocks: CommandBlock[]): CommandBlockTracker & { fire: () => void } {
  const listeners = new Set<() => void>();
  return {
    get blocks() {
      return blocks;
    },
    onChange(fn: () => void) {
      listeners.add(fn);
      return { dispose: () => listeners.delete(fn) };
    },
    dispose() {
      listeners.clear();
    },
    fire() {
      for (const fn of listeners) fn();
    },
  };
}

function makeBlock(
  id: string,
  start: FakeMarker,
  end: FakeMarker | null,
): CommandBlock {
  return {
    id,
    sequence: Number(id),
    color: "hsl(0,0%,50%)",
    command: null,
    startedAt: "",
    endedAt: end ? "ts" : null,
    start: start as unknown as CommandBlock["start"],
    end: end as unknown as CommandBlock["end"],
  };
}

/* ─────────────────────────────────────────────────────────────
 * Tests
 * ───────────────────────────────────────────────────────────── */

describe("FoldStore — fold() validation", () => {
  it("refuses unknown blockId", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const store = createFoldStore(f.term, fakeTracker([]));
    expect(store.fold("99")).toBe(false);
    store.dispose();
  });

  it("refuses block without end (open block)", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const start = f.makeMarker(0);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", start, null)]));
    expect(store.fold("1")).toBe(false);
    store.dispose();
  });

  it("refuses block with empty body", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const s = f.makeMarker(5);
    const e = f.makeMarker(5);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s, e)]));
    expect(store.fold("1")).toBe(false);
    store.dispose();
  });

  it("refuses if fold range overlaps cursor", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 5 });
    const s = f.makeMarker(0);
    const e = f.makeMarker(10);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s, e)]));
    expect(store.fold("1")).toBe(false);
    store.dispose();
  });

  it("refuses double-fold of the same block", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const s = f.makeMarker(0);
    const e = f.makeMarker(12);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s, e)]));
    expect(store.fold("1")).toBe(true);
    expect(store.fold("1")).toBe(false);
    store.dispose();
  });
});

describe("FoldStore — fold() effects", () => {
  it("fold preserves lines.length (push blanks compensates splice)", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const s = f.makeMarker(0);
    const e = f.makeMarker(12);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s, e)]));
    const before = f.snapshot();
    store.fold("1");
    expect(f.snapshot().length).toBe(before.length);
    store.dispose();
  });

  it("fold preserves cursor's content position (cursorAbs -= count)", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const s = f.makeMarker(0);
    const e = f.makeMarker(12);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s, e)]));
    const before = f.snapshot();
    store.fold("1");
    expect(f.snapshot().cursorAbs).toBe(before.cursorAbs - 12);
    store.dispose();
  });

  it("fold disposes block.end (inside splice range) but not start", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const s = f.makeMarker(0);
    const e = f.makeMarker(12);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s, e)]));
    store.fold("1");
    expect(e.isDisposed).toBe(true);
    expect(s.isDisposed).toBe(false);
    store.dispose();
  });

  it("fold auto-migrates markers AFTER the splice range", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 20 });
    const s1 = f.makeMarker(0);
    const e1 = f.makeMarker(12);
    const s2 = f.makeMarker(13);
    const e2 = f.makeMarker(15);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s1, e1), makeBlock("2", s2, e2)]));
    store.fold("1");
    expect(s2.line).toBe(1);
    expect(e2.line).toBe(3);
    store.dispose();
  });

  it("isFolded reflects state", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const s = f.makeMarker(0);
    const e = f.makeMarker(12);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s, e)]));
    expect(store.isFolded("1")).toBe(false);
    store.fold("1");
    expect(store.isFolded("1")).toBe(true);
    store.dispose();
  });
});

describe("FoldStore — unfold() effects", () => {
  it("unfold restores buffer length to pre-fold (safe path)", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const s = f.makeMarker(0);
    const e = f.makeMarker(12);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s, e)]));
    const before = f.snapshot();
    store.fold("1");
    store.unfold("1");
    expect(f.snapshot().length).toBe(before.length);
    store.dispose();
  });

  it("unfold with scrollback restores length precisely", () => {
    const f = fakeTerm({ rows: 24, initialLines: 38, cursorY: 23, ybase: 14 });
    const s = f.makeMarker(20);
    const e = f.makeMarker(30);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s, e)]));
    const before = f.snapshot();
    store.fold("1");
    expect(f.snapshot().ybase).toBe(4);
    store.unfold("1");
    expect(f.snapshot().length).toBe(before.length);
    expect(f.snapshot().ybase).toBe(before.ybase);
    expect(f.snapshot().cursorAbs).toBe(before.cursorAbs);
    store.dispose();
  });

  it("unfold preserves cursor content position", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const s = f.makeMarker(0);
    const e = f.makeMarker(12);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s, e)]));
    const before = f.snapshot();
    store.fold("1");
    store.unfold("1");
    expect(f.snapshot().cursorAbs).toBe(before.cursorAbs);
    store.dispose();
  });

  it("unfold re-registers block.end at correct line", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const s = f.makeMarker(0);
    const e = f.makeMarker(12);
    const block = makeBlock("1", s, e);
    const store = createFoldStore(f.term, fakeTracker([block]));
    store.fold("1");
    expect(block.end?.isDisposed).toBe(true);
    store.unfold("1");
    expect(block.end).not.toBeNull();
    expect(block.end?.isDisposed).toBe(false);
    expect(block.end?.line).toBe(12);
    store.dispose();
  });

  it("unfold returns false for unknown blockId", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const store = createFoldStore(f.term, fakeTracker([]));
    expect(store.unfold("99")).toBe(false);
    store.dispose();
  });

  it("unfold drops fold record if block.start was disposed", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const s = f.makeMarker(0);
    const e = f.makeMarker(12);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s, e)]));
    store.fold("1");
    s.dispose();
    expect(store.unfold("1")).toBe(false);
    expect(store.isFolded("1")).toBe(false);
    store.dispose();
  });
});

describe("FoldStore — multiple folds", () => {
  it("fold two distinct blocks, both tracked independently", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 19 });
    const s1 = f.makeMarker(0);
    const e1 = f.makeMarker(5);
    const s2 = f.makeMarker(6);
    const e2 = f.makeMarker(12);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s1, e1), makeBlock("2", s2, e2)]));
    expect(store.fold("1")).toBe(true);
    expect(store.fold("2")).toBe(true);
    expect(store.isFolded("1")).toBe(true);
    expect(store.isFolded("2")).toBe(true);
    expect(store.folds.length).toBe(2);
    store.dispose();
  });

  it("unfold preserves the OTHER fold's state", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 19 });
    const s1 = f.makeMarker(0);
    const e1 = f.makeMarker(5);
    const s2 = f.makeMarker(6);
    const e2 = f.makeMarker(12);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s1, e1), makeBlock("2", s2, e2)]));
    store.fold("1");
    store.fold("2");
    store.unfold("2");
    expect(store.isFolded("1")).toBe(true);
    expect(store.isFolded("2")).toBe(false);
    store.dispose();
  });

  it("unfolds multiple folds in non-LIFO order without leaking blanks", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 19 });
    const s1 = f.makeMarker(0);
    const e1 = f.makeMarker(5);
    const s2 = f.makeMarker(6);
    const e2 = f.makeMarker(12);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s1, e1), makeBlock("2", s2, e2)]));
    const before = f.snapshot();
    const beforeLines = f.lineContents();

    store.fold("1");
    store.fold("2");
    store.unfold("1");
    store.unfold("2");

    expect(f.snapshot()).toEqual(before);
    expect(f.lineContents()).toEqual(beforeLines);
    store.dispose();
  });
});

describe("FoldStore — auto-cleanup", () => {
  it("unfoldAll expands active folds", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const s = f.makeMarker(0);
    const e = f.makeMarker(12);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s, e)]));
    store.fold("1");
    expect(store.isFolded("1")).toBe(true);
    store.unfoldAll();
    expect(store.isFolded("1")).toBe(false);
    store.dispose();
  });

  it("tracker drops a folded block → fold record auto-dropped", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const s = f.makeMarker(0);
    const e = f.makeMarker(12);
    const blocks = [makeBlock("1", s, e)];
    const tracker = fakeTracker(blocks);
    const store = createFoldStore(f.term, tracker);
    store.fold("1");
    blocks.length = 0;
    tracker.fire();
    expect(store.isFolded("1")).toBe(false);
    expect(store.folds.length).toBe(0);
    store.dispose();
  });

  it("dispose() clears state", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const s = f.makeMarker(0);
    const e = f.makeMarker(12);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s, e)]));
    store.fold("1");
    store.dispose();
    expect(store.folds.length).toBe(0);
  });
});

describe("FoldStore — onChange notifications", () => {
  it("fires onChange on fold/unfold", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const s = f.makeMarker(0);
    const e = f.makeMarker(12);
    const store = createFoldStore(f.term, fakeTracker([makeBlock("1", s, e)]));
    let calls = 0;
    store.onChange(() => calls++);
    store.fold("1");
    expect(calls).toBe(1);
    store.unfold("1");
    expect(calls).toBe(2);
    store.dispose();
  });

  it("does NOT fire onChange on failed fold", () => {
    const f = fakeTerm({ rows: 24, initialLines: 24, cursorY: 14 });
    const store = createFoldStore(f.term, fakeTracker([]));
    let calls = 0;
    store.onChange(() => calls++);
    store.fold("99");
    expect(calls).toBe(0);
    store.dispose();
  });
});
