import { describe, expect, it, vi } from "vitest";
import type { IMarker, Terminal } from "@xterm/xterm";
import { commandBlockColor, createCommandBlockTracker } from "./command-blocks";

function fakeTerminal() {
  const dataListeners = new Set<(data: string) => void>();
  const bufferListeners = new Set<(buffer: { type: "normal" | "alternate" }) => void>();
  const markers: IMarker[] = [];
  let cursorLine = 4;
  let markerId = 0;
  let active: { type: "normal" | "alternate" } = { type: "normal" };
  let escHandler: (() => boolean) | null = null;

  const terminal = {
    buffer: {
      get active() {
        return active;
      },
      onBufferChange(listener: (buffer: typeof active) => void) {
        bufferListeners.add(listener);
        return { dispose: () => bufferListeners.delete(listener) };
      },
    },
    parser: {
      registerEscHandler(_id: unknown, handler: () => boolean) {
        escHandler = handler;
        return {
          dispose: () => {
            escHandler = null;
          },
        };
      },
    },
    onData(listener: (data: string) => void) {
      dataListeners.add(listener);
      return { dispose: () => dataListeners.delete(listener) };
    },
    registerMarker(offset: number) {
      const disposeListeners = new Set<() => void>();
      const marker = {
        id: ++markerId,
        line: cursorLine + offset,
        isDisposed: false,
        onDispose(listener: () => void) {
          disposeListeners.add(listener);
          return { dispose: () => disposeListeners.delete(listener) };
        },
        dispose() {
          if (this.isDisposed) return;
          this.isDisposed = true;
          this.line = -1;
          for (const listener of disposeListeners) listener();
        },
      };
      markers.push(marker as unknown as IMarker);
      return marker as unknown as IMarker;
    },
  } as unknown as Terminal;

  return {
    terminal,
    markers,
    emitData(data: string) {
      for (const listener of dataListeners) listener(data);
    },
    setCursor(line: number) {
      cursorLine = line;
    },
    switchBuffer(type: "normal" | "alternate") {
      active = { type };
      for (const listener of bufferListeners) listener(active);
    },
    reset() {
      return escHandler?.();
    },
  };
}

describe("createCommandBlockTracker", () => {
  it("splits on Enter in the normal buffer and captures best-effort commands", () => {
    const fake = fakeTerminal();
    let pending = "uname -a";
    const tracker = createCommandBlockTracker(fake.terminal, {
      idPrefix: "session",
      getPendingCommand: () => pending,
      now: () => "2026-08-13T00:00:00.000Z",
    });

    fake.emitData("\r");
    expect(tracker.blocks[0]).toMatchObject({
      id: "session:1",
      command: "uname -a",
      endedAt: null,
      start: { line: 4 },
    });

    fake.setCursor(8);
    pending = "pwd";
    fake.emitData("\r");
    expect(tracker.blocks).toHaveLength(2);
    expect(tracker.blocks[0].end?.line).toBe(7);
    expect(tracker.blocks[0].endedAt).not.toBeNull();
    expect(tracker.blocks[1].command).toBe("pwd");
  });

  it("ignores alternate-buffer Enter and closes with a stable normal marker", () => {
    const fake = fakeTerminal();
    const tracker = createCommandBlockTracker(fake.terminal, { getPendingCommand: () => "top" });
    fake.emitData("\r");
    const start = tracker.blocks[0].start;

    fake.switchBuffer("alternate");
    expect(tracker.blocks[0].end).toBe(start);
    fake.emitData("\r");
    expect(tracker.blocks).toHaveLength(1);
  });

  it("drops trimmed blocks, resets on RIS, and cleans every marker", () => {
    const fake = fakeTerminal();
    const changed = vi.fn();
    const tracker = createCommandBlockTracker(fake.terminal);
    tracker.onChange(changed);
    fake.emitData("\r");
    fake.setCursor(6);
    fake.emitData("\r");
    tracker.blocks[0].start.dispose();
    expect(tracker.blocks).toHaveLength(1);

    expect(fake.reset()).toBe(false);
    expect(tracker.blocks).toHaveLength(0);
    expect(changed).toHaveBeenCalled();
    tracker.dispose();
    expect(fake.markers.every((marker) => marker.isDisposed)).toBe(true);
  });

  it("uses a deterministic golden-angle palette", () => {
    expect(commandBlockColor(1)).toBe("hsl(137.5, 68%, 58%)");
    expect(commandBlockColor(2)).toBe("hsl(275.0, 68%, 58%)");
    expect(commandBlockColor(1)).not.toBe(commandBlockColor(2));
  });
});
