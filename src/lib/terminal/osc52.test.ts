import { Terminal } from "@xterm/xterm";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { registerClipboardOscHandler } from "./osc52";

function base64Utf8(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

function setup() {
  const terminal = new Terminal({ cols: 80, rows: 24 });
  const writeText = vi.fn(async (_text: string) => {});
  const disposable = registerClipboardOscHandler(terminal.parser, { writeText });
  return { terminal, writeText, disposable };
}

function write(terminal: Terminal, data: string | Uint8Array): Promise<void> {
  return new Promise((resolve) => terminal.write(data, resolve));
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("registerClipboardOscHandler", () => {
  it("reassembles a split OSC 52 sequence and keeps it out of terminal output", async () => {
    const { terminal, writeText } = setup();
    const payload = base64Utf8("hello 中 👋");

    await write(terminal, `before\x1b]52;c;${payload.slice(0, 5)}`);
    await write(terminal, `${payload.slice(5)}\x1b`);
    await write(terminal, "\\after");

    expect(writeText).toHaveBeenCalledWith("hello 中 👋");
    expect(terminal.buffer.active.getLine(0)?.translateToString(true)).toBe("beforeafter");
    terminal.dispose();
  });

  it("supports BEL termination, an empty selector, empty writes, and unpadded base64", async () => {
    const { terminal, writeText } = setup();

    await write(terminal, `\x1b]52;;${base64Utf8("pa").replace(/=+$/, "")}\x07`);
    await write(terminal, "\x1b]52;c;\x07");

    expect(writeText).toHaveBeenNthCalledWith(1, "pa");
    expect(writeText).toHaveBeenNthCalledWith(2, "");
    terminal.dispose();
  });

  it("ignores read queries and selectors that do not address the system clipboard", async () => {
    const { terminal, writeText } = setup();

    await write(terminal, "\x1b]52;c;?\x07");
    await write(terminal, `\x1b]52;p;${base64Utf8("primary")}\x07`);

    expect(writeText).not.toHaveBeenCalled();
    terminal.dispose();
  });

  it("rejects malformed base64, invalid UTF-8, and payloads above one MiB", async () => {
    const { terminal, writeText } = setup();

    await write(terminal, "\x1b]52;c;not*base64\x07");
    await write(terminal, "\x1b]52;c;/w==\x07");
    await write(terminal, `\x1b]52;c;${"A".repeat(1_400_000)}\x07`);

    expect(writeText).not.toHaveBeenCalled();
    terminal.dispose();
  });

  it("contains synchronous and asynchronous clipboard failures", async () => {
    const terminal = new Terminal();
    const syncFailure = registerClipboardOscHandler(terminal.parser, {
      writeText: () => {
        throw new Error("denied");
      },
    });
    await expect(write(terminal, `\x1b]52;c;${base64Utf8("sync")}\x07`)).resolves.toBeUndefined();
    syncFailure.dispose();

    const asyncFailure = registerClipboardOscHandler(terminal.parser, {
      writeText: async () => {
        throw new Error("denied");
      },
    });
    await expect(write(terminal, `\x1b]52;c;${base64Utf8("async")}\x07`)).resolves.toBeUndefined();
    asyncFailure.dispose();
    terminal.dispose();
  });
});
