/**
 * FoldStore — command-block fold/unfold (real buffer splice, not CSS hide).
 *
 * Absorbed from RSSH's `src/lib/terminal/folds.ts`. A folded block's body
 * rows are physically removed from xterm's CircularList via `lines.splice`,
 * then re-inserted on unfold, so the folded terminal is indistinguishable
 * from an unfolded one to xterm (scroll, selection, copy, search all work).
 *
 * Design invariants (spike-verified against @xterm/xterm 6.0.0):
 *   1. xterm Buffer.addMarker registers lines.onDelete/onInsert/onTrim:
 *      splice migrates marker line numbers and disposes in-range markers.
 *   2. Hidden invariant `lines.length === ybase + rows`: after a splice we
 *      must pad with `Buffer.getBlankLine` at the tail.
 *   3. Cursor content must follow: when the splice is above the cursor, the
 *      cursor's absolute line decreases (fold) / increases (unfold).
 *
 * ⚠️ Private-API warning: depends on `_core.buffer`'s lines/ybase/ydisp/y,
 *   getBlankLine/addMarker, plus `_core._viewport.queueSync` (scrollbar
 *   resync). package.json pins "@xterm/xterm": "^6.0.0". Any xterm bump must
 *   re-run folds.test.ts and re-check these private hooks by hand.
 */
import type { Terminal, IDisposable, IMarker } from "@xterm/xterm";
import type { CommandBlock, CommandBlockTracker } from "./command-blocks";

export interface Fold {
  /** 自增 id（仅用于调试）；外界以 blockId 索引 */
  id: number;
  blockId: string;
  /** full = user folded the complete closed block; prefix = automatic oldest rows. */
  kind: "full" | "prefix";
  /** body 行数 */
  count: number;
  /** splice 抽出的 BufferLine 实例（对我们透明） */
  savedLines: unknown[];
  /** 这次 fold push 进 buffer 末尾的空行 refs。 */
  pushedBlankRefs: unknown[];
}

interface FoldState extends Fold {
  /** Number of leading compensation lines the output cursor has reached. */
  consumedBlankCount: number;
}

export interface FoldStore extends IDisposable {
  readonly folds: ReadonlyArray<Fold>;
  fold(blockId: string): boolean;
  unfold(blockId: string): boolean;
  isFolded(blockId: string): boolean;
  /** O(1) 取 fold 记录。 */
  getFold(blockId: string): Fold | undefined;
  /** Expand every fold before xterm changes row/column geometry. */
  unfoldAll(): void;
  /** Reapply the configured automatic limit after xterm reflows its rows. */
  enforceAutoFold(): void;
  /** 折叠状态变化时通知（fold/unfold/scrollback 失效）。 */
  onChange(fn: () => void): IDisposable;
}

export interface FoldStoreOptions {
  /** Keep this many newest body rows visible in a growing command block. */
  maxVisibleLines?: number;
  /** Global saved-line budget across every fold in this terminal. */
  maxCachedLines?: number;
  /** Visual block controls must be available before automatic folding hides rows. */
  shouldAutoFold?: () => boolean;
}

/** xterm 默认 attr（fg=0,bg=0），与 DEFAULT_ATTR_DATA 等价。getBlankLine 必填。 */
const BLANK_ATTR = { fg: 0, bg: 0, extended: { ext: 0, urlId: 0, underlineStyle: 0 } };
// Bound work inside one xterm parse batch without splicing on every LF.
const AUTO_FOLD_BATCH_LINES = 32;

interface PrivateBuffer {
  lines: {
    length: number;
    get(i: number): unknown;
    splice(start: number, deleteCount: number, ...items: unknown[]): void;
    push(item: unknown): void;
  };
  ybase: number;
  ydisp: number;
  y: number;
  getBlankLine(attr: unknown): unknown;
  addMarker(line: number): IMarker;
}

interface PrivateViewport {
  queueSync(yDisp?: number): void;
}

function getBuf(term: Terminal): PrivateBuffer {
  const core = (term as unknown as {
    _core: { buffer: PrivateBuffer; buffers?: { normal: PrivateBuffer } };
  })._core;
  return core.buffers?.normal ?? core.buffer;
}

/** We splice buffer.lines directly, bypassing the core's scroll/resize events,
 *  so the scrollbar would go stale. queueSync() recomputes it on the next
 *  render frame; folds always calls term.refresh() right after, driving it. */
function syncViewport(term: Terminal): void {
  const vp = (term as unknown as { _core: { _viewport?: PrivateViewport } })._core._viewport;
  vp?.queueSync();
}

function clamp(n: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, n));
}

function indexLineRefs(lines: PrivateBuffer["lines"], refs: Iterable<unknown>): Map<unknown, number> {
  const targets = new Set(refs);
  const indices = new Map<unknown, number>();
  if (targets.size === 0) return indices;

  for (let i = 0; i < lines.length; i++) {
    const line = lines.get(i);
    if (targets.has(line)) {
      indices.set(line, i);
      if (indices.size === targets.size) break;
    }
  }
  return indices;
}

function isStillBlankLine(line: unknown): boolean {
  const candidate = line as { getTrimmedLength?: () => number; isWrapped?: boolean } | null;
  if (candidate && typeof candidate.getTrimmedLength === "function") {
    return candidate.getTrimmedLength() === 0 && candidate.isWrapped !== true;
  }
  return true;
}

export function createFoldStore(
  term: Terminal,
  tracker: CommandBlockTracker,
  options: FoldStoreOptions = {},
): FoldStore {
  const folds = new Map<string, FoldState>();
  const blankOwners = new Map<unknown, { blockId: string; index: number }>();
  const listeners = new Set<() => void>();
  const disposables: IDisposable[] = [];
  let nextId = 1;

  const emit = () => {
    for (const fn of listeners) fn();
  };

  const maxVisibleLines = Number.isFinite(options.maxVisibleLines)
    ? Math.max(1, Math.trunc(options.maxVisibleLines!))
    : null;
  const maxCachedLines = Number.isFinite(options.maxCachedLines)
    ? Math.max(1, Math.trunc(options.maxCachedLines!))
    : Number.POSITIVE_INFINITY;

  function discardFold(f: FoldState): void {
    for (const line of f.pushedBlankRefs) {
      const owner = blankOwners.get(line);
      if (owner?.blockId === f.blockId) blankOwners.delete(line);
    }
  }

  function pruneConsumedBlankRefs(f: FoldState): void {
    const consumed = Math.min(f.consumedBlankCount, f.pushedBlankRefs.length);
    if (consumed === 0) return;
    for (const line of f.pushedBlankRefs.slice(0, consumed)) {
      const owner = blankOwners.get(line);
      if (owner?.blockId === f.blockId) blankOwners.delete(line);
    }
    f.pushedBlankRefs.splice(0, consumed);
    f.consumedBlankCount = 0;
    for (let index = 0; index < f.pushedBlankRefs.length; index++) {
      blankOwners.set(f.pushedBlankRefs[index], { blockId: f.blockId, index });
    }
  }

  /** Drop the oldest saved rows first, matching xterm's head-trim policy. */
  function enforceSavedLineBudget(currentBlockId: string): void {
    if (!Number.isFinite(maxCachedLines)) return;
    let overflow = Array.from(folds.values())
      .reduce((total, item) => total + item.savedLines.length, 0) - maxCachedLines;
    if (overflow <= 0) return;

    for (const blockId of Array.from(folds.keys())) {
      if (blockId === currentBlockId) continue;
      unfold(blockId);
      overflow = Array.from(folds.values())
        .reduce((total, item) => total + item.savedLines.length, 0) - maxCachedLines;
      if (overflow <= 0) return;
    }

    const current = folds.get(currentBlockId);
    if (!current || overflow <= 0) return;
    current.savedLines.splice(0, Math.min(overflow, current.savedLines.length));
    current.count = current.savedLines.length;
  }

  function removeLines(
    blockId: string,
    kind: Fold["kind"],
    startLine: number,
    count: number,
  ): boolean {
    if (count <= 0) return false;
    const existing = folds.get(blockId);
    if (existing && (existing.kind !== "prefix" || kind !== "prefix")) return false;
    if (existing) {
      recordCursorConsumption();
      for (let i = existing.consumedBlankCount; i < existing.pushedBlankRefs.length; i++) {
        if (!isStillBlankLine(existing.pushedBlankRefs[i])) {
          existing.consumedBlankCount = i + 1;
        }
      }
      pruneConsumedBlankRefs(existing);
    }

    const buf = getBuf(term);
    const cursorAbs = buf.ybase + buf.y;
    const endLine = startLine + count - 1;
    if (endLine >= cursorAbs) return false;
    const wasLive = buf.ydisp === buf.ybase;

    const saved: unknown[] = [];
    for (let i = 0; i < count; i++) saved.push(buf.lines.get(startLine + i));

    const ybaseDrain = Math.min(buf.ybase, count);
    const pushCount = count - ybaseDrain;
    buf.lines.splice(startLine, count);

    const pushedRefs: unknown[] = [];
    for (let i = 0; i < pushCount; i++) {
      const blank = buf.getBlankLine(BLANK_ATTR);
      buf.lines.push(blank);
      pushedRefs.push(blank);
    }

    buf.ybase -= ybaseDrain;
    buf.y = Math.max(0, buf.y - pushCount);
    if (buf.ydisp >= startLine + count) buf.ydisp -= count;
    else if (buf.ydisp >= startLine) buf.ydisp = startLine;
    if (buf.ydisp > buf.ybase) buf.ydisp = buf.ybase;
    if (wasLive) buf.ydisp = buf.ybase;

    const foldState = existing ?? {
      id: nextId++,
      blockId,
      kind,
      count: 0,
      savedLines: [],
      pushedBlankRefs: [],
      consumedBlankCount: 0,
    };
    foldState.savedLines.push(...saved);
    foldState.count = foldState.savedLines.length;
    const blankOffset = foldState.pushedBlankRefs.length;
    foldState.pushedBlankRefs.push(...pushedRefs);
    folds.set(blockId, foldState);
    for (let index = 0; index < pushedRefs.length; index++) {
      blankOwners.set(pushedRefs[index], { blockId, index: blankOffset + index });
    }
    enforceSavedLineBudget(blockId);
    syncViewport(term);
    term.refresh(0, term.rows - 1);
    emit();
    return true;
  }

  function fold(blockId: string): boolean {
    if (folds.has(blockId)) return false;
    const block = tracker.blocks.find((b) => b.id === blockId);
    if (!block || !block.end) return false;
    if (block.start.isDisposed || block.end.isDisposed) return false;
    const startLine = block.start.line + 1;
    const endLine = block.end.line;
    if (startLine > endLine) return false; // 空 body

    const count = endLine - startLine + 1;
    return removeLines(blockId, "full", startLine, count);
  }

  function canAutoFold(): boolean {
    return maxVisibleLines !== null
      && options.shouldAutoFold?.() !== false
      && term.buffer.active.type === "normal";
  }

  function foldBlockOverflow(block: CommandBlock, minimumExcess: number): void {
    if (maxVisibleLines === null || block.start.isDisposed) return;
    if (folds.get(block.id)?.kind === "full") return;
    const buf = getBuf(term);
    const endLine = block.end === null
      ? buf.ybase + buf.y
      : block.end.isDisposed ? null : block.end.line;
    if (endLine === null) return;
    const visibleBodyLines = endLine - block.start.line;
    const excess = visibleBodyLines - maxVisibleLines;
    if (excess >= minimumExcess) {
      removeLines(block.id, "prefix", block.start.line + 1, excess);
    }
  }

  function foldActiveOverflow(minimumExcess: number): void {
    if (!canAutoFold()) return;
    const block = tracker.blocks[tracker.blocks.length - 1];
    if (block?.end === null) foldBlockOverflow(block, minimumExcess);
  }

  function foldRecentOverflow(): void {
    if (!canAutoFold()) return;
    const blocks = tracker.blocks;
    for (let i = Math.max(0, blocks.length - 2); i < blocks.length; i++) {
      foldBlockOverflow(blocks[i], 1);
    }
  }

  function enforceAutoFold(): void {
    if (!canAutoFold()) return;
    for (const block of Array.from(tracker.blocks)) foldBlockOverflow(block, 1);
  }

  function recordCursorConsumption(): void {
    if (folds.size === 0) return;
    if (term.buffer.active.type !== "normal") return;
    const buf = getBuf(term);
    const cursorAbs = buf.ybase + buf.y;
    const owner = blankOwners.get(buf.lines.get(cursorAbs));
    if (!owner) return;
    const fold = folds.get(owner.blockId);
    if (fold && owner.index >= fold.consumedBlankCount) {
      fold.consumedBlankCount = owner.index + 1;
    }
  }

  function unfold(blockId: string): boolean {
    const f = folds.get(blockId);
    if (!f) return false;
    const block = tracker.blocks.find((b) => b.id === blockId);
    if (!block || block.start.isDisposed) {
      discardFold(f);
      folds.delete(blockId);
      emit();
      return false;
    }
    const buf = getBuf(term);
    const insertAt = block.start.line + 1;
    const cursorAbsBefore = buf.ybase + buf.y;
    let nextCursorAbs = insertAt <= cursorAbsBefore ? cursorAbsBefore + f.count : cursorAbsBefore;
    let nextYdisp = buf.ydisp;
    const wasLive = buf.ydisp === buf.ybase;

    recordCursorConsumption();
    for (let i = f.consumedBlankCount; i < f.pushedBlankRefs.length; i++) {
      if (!isStillBlankLine(f.pushedBlankRefs[i])) f.consumedBlankCount = i + 1;
    }
    const blankRefIndicesBeforeInsert = indexLineRefs(buf.lines, f.pushedBlankRefs);
    const untouchedBlankRefs = new Set(
      f.pushedBlankRefs.slice(f.consumedBlankCount).filter((line) => {
        const index = blankRefIndicesBeforeInsert.get(line);
        return index !== undefined && index > cursorAbsBefore && isStillBlankLine(line);
      }),
    );

    // 分块插：Array spread 在 V8 上有 ~65k 参数硬上限。
    const SPLICE_CHUNK = 32768;
    let inserted = 0;
    let trimmedDuringInsert = 0;
    for (let i = 0; i < f.savedLines.length; i += SPLICE_CHUNK) {
      const chunk = f.savedLines.slice(i, i + SPLICE_CHUNK);
      const chunkInsertAt = clamp(
        insertAt + inserted - trimmedDuringInsert,
        0,
        buf.lines.length,
      );
      const lengthBeforeChunk = buf.lines.length;
      buf.lines.splice(chunkInsertAt, 0, ...chunk);
      trimmedDuringInsert += Math.max(
        0,
        lengthBeforeChunk + chunk.length - buf.lines.length,
      );
      inserted += chunk.length;
    }
    if (insertAt <= nextYdisp) nextYdisp += f.count;

    if (trimmedDuringInsert > 0) {
      nextCursorAbs = Math.max(0, nextCursorAbs - trimmedDuringInsert);
      nextYdisp = Math.max(0, nextYdisp - trimmedDuringInsert);
    }

    const removableIndices = indexLineRefs(buf.lines, untouchedBlankRefs);
    const removable = Array.from(untouchedBlankRefs)
      .map((line) => ({ line, index: removableIndices.get(line) }))
      .filter((item): item is { line: unknown; index: number } => item.index !== undefined && isStillBlankLine(item.line))
      .sort((a, b) => b.index - a.index);

    for (const { index } of removable) {
      buf.lines.splice(index, 1);
      if (index < nextCursorAbs) nextCursorAbs--;
      if (index < nextYdisp) nextYdisp--;
    }

    while (buf.lines.length < term.rows) {
      buf.lines.push(buf.getBlankLine(BLANK_ATTR));
    }

    if (block.start.isDisposed || block.start.line < 0) {
      buf.ybase = Math.max(0, buf.lines.length - term.rows);
      buf.y = clamp(nextCursorAbs - buf.ybase, 0, term.rows - 1);
      buf.ydisp = wasLive ? buf.ybase : clamp(nextYdisp, 0, buf.ybase);
      discardFold(f);
      folds.delete(blockId);
      syncViewport(term);
      term.refresh(0, term.rows - 1);
      emit();
      return false;
    }

    buf.ybase = Math.max(0, buf.lines.length - term.rows);
    buf.y = clamp(nextCursorAbs - buf.ybase, 0, term.rows - 1);
    buf.ydisp = wasLive ? buf.ybase : clamp(nextYdisp, 0, buf.ybase);

    if (f.kind === "full") {
      try {
        const newEnd = buf.addMarker(block.start.line + f.count);
        block.end = newEnd;
      } catch {
        // Keep the disposed marker; renderers already fall back to the cursor.
      }
    }

    discardFold(f);
    folds.delete(blockId);
    syncViewport(term);
    term.refresh(0, term.rows - 1);
    emit();
    return true;
  }

  function unfoldAll(): void {
    for (const blockId of Array.from(folds.keys())) unfold(blockId);
  }

  disposables.push(
    term.onCursorMove(recordCursorConsumption),
    term.onLineFeed(() => {
      recordCursorConsumption();
      foldActiveOverflow(AUTO_FOLD_BATCH_LINES);
    }),
  );
  if (maxVisibleLines !== null) {
    disposables.push(term.onWriteParsed(foldRecentOverflow));
  }

  disposables.push(tracker.onChange(() => {
    const trackedIds = new Set(tracker.blocks.map((b) => b.id));
    let dropped = false;
    for (const blockId of Array.from(folds.keys())) {
      if (!trackedIds.has(blockId)) {
        const fold = folds.get(blockId);
        if (fold) discardFold(fold);
        folds.delete(blockId);
        dropped = true;
      }
    }
    if (dropped) emit();
  }));

  return {
    get folds() {
      return Array.from(folds.values());
    },
    fold,
    unfold,
    isFolded(blockId) {
      return folds.has(blockId);
    },
    getFold(blockId) {
      return folds.get(blockId);
    },
    unfoldAll,
    enforceAutoFold,
    onChange(fn) {
      listeners.add(fn);
      return { dispose: () => listeners.delete(fn) };
    },
    dispose() {
      for (const d of disposables) d.dispose();
      folds.clear();
      blankOwners.clear();
      listeners.clear();
    },
  };
}
