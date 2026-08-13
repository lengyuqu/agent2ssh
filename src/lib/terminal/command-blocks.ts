import type { IDisposable, IMarker, Terminal } from "@xterm/xterm";
import { detectPrompt } from "./prompt";

/** How a command block boundary is detected. */
export type CommandBlockSplitMode = "enter" | "prompt";

/** A command block is anchored entirely with xterm's public marker API. */
export interface CommandBlock {
  id: string;
  sequence: number;
  color: string;
  command: string | null;
  startedAt: string;
  endedAt: string | null;
  start: IMarker;
  end: IMarker | null;
}

export interface CommandBlockTracker extends IDisposable {
  readonly blocks: ReadonlyArray<CommandBlock>;
  onChange(listener: () => void): IDisposable;
}

export interface CommandBlockTrackerOptions {
  idPrefix?: string;
  /** Read immediately before TerminalView clears its best-effort input buffer. */
  getPendingCommand?: () => string;
  now?: () => string;
  /**
   * "enter" (default) splits on every submitted Enter — deterministic.
   * "prompt" additionally waits for the returned shell prompt before closing
   * the block, which yields boundaries closer to real command completion.
   * Prompt detection is fully local and may miss or mis-detect, hence the
   * deterministic Enter mode remains the default.
   */
  splitMode?: CommandBlockSplitMode;
}

/** Golden-angle cycling avoids adjacent blocks receiving similar hues. */
export function commandBlockColor(index: number): string {
  const hue = (index * 137.508) % 360;
  return `hsl(${hue.toFixed(1)}, 68%, 58%)`;
}

export function createCommandBlockTracker(
  terminal: Terminal,
  options: CommandBlockTrackerOptions = {},
): CommandBlockTracker {
  const blocks: CommandBlock[] = [];
  const listeners = new Set<() => void>();
  const disposables: IDisposable[] = [];
  const now = options.now ?? (() => new Date().toISOString());
  const prefix = options.idPrefix ?? "block";
  const splitMode = options.splitMode ?? "enter";
  let nextSequence = 1;
  let waitingForPrompt = splitMode === "prompt";
  let submittedLine: IMarker | null = null;

  const emit = () => {
    for (const listener of listeners) listener();
  };

  /** The logical line at the cursor (soft-wrapped rows joined back together). */
  const logicalLineAtCursor = (): { text: string; startLine: number } | null => {
    const buf = terminal.buffer.active;
    const cursorAbs = buf.baseY + buf.cursorY;
    let startAbs = cursorAbs;
    while (startAbs > 0 && buf.getLine(startAbs)?.isWrapped) startAbs--;
    let text = "";
    for (let y = startAbs; y <= cursorAbs; y++) {
      const line = buf.getLine(y);
      if (!line) return null;
      text += line.translateToString(true);
    }
    return { text, startLine: startAbs };
  };

  const closeCurrent = (endOffset = -1) => {
    const current = blocks[blocks.length - 1];
    if (!current || current.end) return;

    // Buffer-change fires after xterm has activated the alternate buffer. A
    // marker created there would be destroyed on return, so close at the last
    // stable normal-buffer marker instead.
    if (terminal.buffer.active.type === "alternate") {
      current.end = current.start;
    } else {
      current.end = terminal.registerMarker(endOffset) ?? terminal.registerMarker(0);
    }
    current.endedAt = now();
    emit();
  };

  const open = (command: string | null) => {
    const start = terminal.registerMarker(0);
    if (!start) return;
    const sequence = nextSequence++;
    const block: CommandBlock = {
      id: `${prefix}:${sequence}`,
      sequence,
      color: commandBlockColor(sequence),
      command,
      startedAt: now(),
      endedAt: null,
      start,
      end: null,
    };
    start.onDispose(() => {
      const index = blocks.indexOf(block);
      if (index < 0) return;
      blocks.splice(index, 1);
      if (block.end && block.end !== block.start && !block.end.isDisposed) block.end.dispose();
      emit();
    });
    blocks.push(block);
    emit();
  };

  const split = () => {
    closeCurrent(-1);
    const pending = options.getPendingCommand?.().trim() ?? "";
    open(pending || null);
  };

  const clearSubmittedLine = () => {
    submittedLine?.dispose();
    submittedLine = null;
  };

  // Prompt mode: after a submitted Enter, wait for the returned shell prompt
  // before closing the block. The submitted line is remembered so we don't
  // re-detect the prompt we just sent.
  const waitForReturnedPrompt = () => {
    const line = logicalLineAtCursor();
    clearSubmittedLine();
    submittedLine = line ? terminal.registerMarker(line.startLine - (terminal.buffer.active.baseY + terminal.buffer.active.cursorY)) : null;
    waitingForPrompt = true;
  };

  const reset = () => {
    clearSubmittedLine();
    waitingForPrompt = splitMode === "prompt";
    if (blocks.length === 0) return;
    const snapshot = blocks.slice();
    blocks.length = 0;
    for (const block of snapshot) {
      if (!block.start.isDisposed) block.start.dispose();
      if (block.end && block.end !== block.start && !block.end.isDisposed) block.end.dispose();
    }
    emit();
  };

  disposables.push(
    terminal.onData((data) => {
      if (terminal.buffer.active.type !== "normal") return;
      for (const character of data) {
        if (character === "\r") {
          if (splitMode === "enter") split();
          else waitForReturnedPrompt();
        }
      }
    }),
    terminal.buffer.onBufferChange((buffer) => {
      if (buffer.type === "alternate") closeCurrent();
    }),
    terminal.parser.registerEscHandler({ final: "c" }, () => {
      reset();
      return false;
    }),
  );

  if (splitMode === "prompt") {
    disposables.push(
      terminal.onWriteParsed(() => {
        if (!waitingForPrompt || terminal.buffer.active.type === "alternate") return;
        const line = logicalLineAtCursor();
        if (!line) return;
        const cursorAbs = terminal.buffer.active.baseY + terminal.buffer.active.cursorY;
        if (submittedLine && !submittedLine.isDisposed && submittedLine.line === cursorAbs) return;
        const prompt = detectPrompt(line.text);
        if (!prompt || line.text.slice(prompt.end).trim().length > 0) return;
        split();
        waitingForPrompt = false;
        clearSubmittedLine();
      }),
    );
  }

  return {
    get blocks() {
      return blocks;
    },
    onChange(listener) {
      listeners.add(listener);
      return { dispose: () => listeners.delete(listener) };
    },
    dispose() {
      for (const disposable of disposables) disposable.dispose();
      reset();
      listeners.clear();
    },
  };
}
