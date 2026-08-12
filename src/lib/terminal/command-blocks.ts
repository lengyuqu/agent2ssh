import type { IDisposable, IMarker, Terminal } from "@xterm/xterm";

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
  let nextSequence = 1;

  const emit = () => {
    for (const listener of listeners) listener();
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

  const reset = () => {
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
        if (character === "\r") split();
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
