import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { api, getDaemonUrl } from "../api";
import type { TerminalThemeId } from "../terminalThemes";
import { resolveTerminalTheme } from "../terminalThemes";
import type { Theme as AppTheme } from "../theme";
import { compileHighlightRules } from "../lib/terminal/highlight";
import { TerminalHighlightDecorations } from "../lib/terminal/highlight-decorations";

/**
 * T1-8: Parse and handle OSC 52 clipboard sequences from terminal output.
 *
 * OSC 52 format: ESC ] 52 ; <clipboard> ; <base64-data> ST
 *   - clipboard: 'c' for system clipboard, 'p' for primary (X11)
 *   - ST: either BEL (0x07) or ESC \ (0x1b 0x5c)
 *
 * When a valid OSC 52 sequence is found, the base64 payload is decoded and
 * written to the system clipboard via `navigator.clipboard.writeText()`.
 * Empty payloads clear the clipboard.
 *
 * This is a standard SSH client feature — remote programs like vim, tmux,
 * and pbcopy-on-Linux use it to write to the user's local clipboard.
 */
function extractOsc52(data: string): { clipboard: string; rest: string } | null {
  const osc52Start = data.indexOf("\x1b]52;");
  if (osc52Start === -1) return null;

  const afterStart = data.slice(osc52Start + 4); // skip ESC ] 5 2 ;
  // Parse clipboard target + semicolon
  const semiIdx = afterStart.indexOf(";");
  if (semiIdx === -1 || semiIdx > 2) return null;

  const clipboardTarget = afterStart.slice(0, semiIdx);
  if (!["c", "p", "s", "0", "1", "2"].includes(clipboardTarget)) return null;

  const payloadStart = semiIdx + 1;
  // Find the terminator: BEL (0x07) or ESC \ (0x1b 0x5c)
  let endIdx = -1;
  let terminatorLen = 0;
  for (let i = payloadStart; i < afterStart.length; i++) {
    if (afterStart[i] === "\x07") {
      endIdx = i;
      terminatorLen = 1;
      break;
    }
    if (afterStart[i] === "\x1b" && i + 1 < afterStart.length && afterStart[i + 1] === "\\") {
      endIdx = i;
      terminatorLen = 2;
      break;
    }
  }
  if (endIdx === -1) return null;

  const base64Payload = afterStart.slice(payloadStart, endIdx);
  return {
    clipboard: base64Payload,
    rest: data.slice(0, osc52Start) + data.slice(osc52Start + 4 + afterStart.length),
  };
}

/** Decode a base64 OSC 52 payload and write to the system clipboard. */
async function handleOsc52(base64Payload: string): Promise<void> {
  try {
    if (base64Payload.length === 0) {
      // Empty payload = clear clipboard
      await navigator.clipboard.writeText("");
      return;
    }
    // Decode base64 to binary string, then to UTF-8 text
    const binary = atob(base64Payload);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    const text = new TextDecoder().decode(bytes);
    await navigator.clipboard.writeText(text);
  } catch {
    // clipboard.writeText may throw if permissions are denied or the context
    // is not secure (HTTP). Silently ignore — the sequence still passes through.
  }
}

type Props = {
  host: string;
  terminalTheme: TerminalThemeId;
  appTheme: AppTheme;
  /** V3-2: fires once per completed input line the user typed (Enter-terminated),
   *  best-effort — used for the Ctrl+R history search, not a shell parser. */
  onLineTyped?: (line: string) => void;
  /** V3-2: Ctrl+R is intercepted (not forwarded to the remote shell) to open the
   *  app's own history search instead of the shell's native reverse-i-search. */
  onHistoryRequest?: () => void;
};

export type TerminalViewHandle = {
  /** Inject text as if typed, without a trailing Enter — used by history search
   *  so the user can review/edit a past line before running it. */
  sendText: (text: string) => void;
  focus: () => void;
};

/** A live interactive terminal to a host, streamed over the daemon's
 *  /terminal WebSocket (raw bytes both ways: ANSI, control chars, TUIs). */
const TerminalView = forwardRef<TerminalViewHandle, Props>(function TerminalView(
  { host, terminalTheme, appTheme, onLineTyped, onHistoryRequest },
  ref
) {
  const containerRef = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const encoderRef = useRef(new TextEncoder());
  const lineBufferRef = useRef("");
  const onLineTypedRef = useRef(onLineTyped);
  onLineTypedRef.current = onLineTyped;
  const onHistoryRequestRef = useRef(onHistoryRequest);
  onHistoryRequestRef.current = onHistoryRequest;

  useImperativeHandle(
    ref,
    () => ({
      sendText: (text: string) => {
        const ws = wsRef.current;
        if (ws && ws.readyState === WebSocket.OPEN) {
          ws.send(encoderRef.current.encode(text));
        }
        lineBufferRef.current += text;
      },
      focus: () => termRef.current?.focus(),
    }),
    []
  );

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const term = new Terminal({
      fontFamily: '"SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace',
      fontSize: 13,
      fontWeight: 450,
      fontWeightBold: 700,
      lineHeight: 1.18,
      letterSpacing: 0,
      cursorBlink: true,
      cursorStyle: "block",
      scrollback: 8000,
      smoothScrollDuration: 80,
      allowTransparency: false,
      theme: resolveTerminalTheme(terminalTheme, appTheme),
    });
    termRef.current = term;
    const highlighter = new TerminalHighlightDecorations(term, []);
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(container);
    try {
      fit.fit();
    } catch {
      // container may not be measurable yet
    }

    let ws: WebSocket | null = null;
    let disposed = false;
    const encoder = encoderRef.current;
    lineBufferRef.current = "";

    const writeTerminal = (data: string | Uint8Array) => {
      term.write(data, () => highlighter.refresh());
    };
    const writeTerminalLine = (data: string) => {
      term.writeln(data, () => highlighter.refresh());
    };

    const refreshHighlightRules = () => {
      void api
        .listHighlights()
        .then((rules) => highlighter.setRules(compileHighlightRules(rules)))
        .catch(() => highlighter.setRules([]));
    };
    refreshHighlightRules();
    window.addEventListener("agent2ssh:highlights-changed", refreshHighlightRules);
    const scrollSub = term.onScroll(() => highlighter.refresh());

    function sendResize() {
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: "resize", cols: term.cols, rows: term.rows }));
      }
    }

    // V3-2: Ctrl+R normally reaches the remote shell (bash's own reverse-i-search).
    // Grab it before xterm forwards it so the app's history search can take over;
    // every other key passes through unchanged.
    term.attachCustomKeyEventHandler((event) => {
      if (event.type === "keydown" && event.ctrlKey && event.key.toLowerCase() === "r") {
        onHistoryRequestRef.current?.();
        return false;
      }
      return true;
    });

    // Keyboard / paste → PTY (as binary frames). Also feeds a best-effort
    // completed-line buffer for the history search — mirrors the line
    // buffering the backend already does for input authorization (see
    // docs/architecture.md), not a real shell parser.
    const dataSub = term.onData((data) => {
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(encoder.encode(data));
      }
      for (const ch of data) {
        if (ch === "\r" || ch === "\n") {
          const line = lineBufferRef.current.trim();
          lineBufferRef.current = "";
          if (line) onLineTypedRef.current?.(line);
        } else if (ch === "\x7f" || ch === "\b") {
          lineBufferRef.current = lineBufferRef.current.slice(0, -1);
        } else if (ch === "\x15" || ch === "\x03") {
          lineBufferRef.current = "";
        } else if (ch >= " ") {
          lineBufferRef.current += ch;
        }
      }
    });

    (async () => {
      let token = "";
      try {
        await api.daemonStart();
        token = await api.getDaemonToken();
      } catch (err) {
        writeTerminalLine(`\x1b[31mFailed to start daemon: ${String(err)}\x1b[0m`);
        return;
      }
      if (disposed) return;
      const base = getDaemonUrl().replace(/^http/, "ws");
      const url = `${base}/terminal?host=${encodeURIComponent(host)}&token=${encodeURIComponent(token)}`;
      ws = new WebSocket(url);
      wsRef.current = ws;
      ws.binaryType = "arraybuffer";
      ws.onopen = () => {
        sendResize();
      };
      ws.onmessage = (ev) => {
        if (typeof ev.data === "string") {
          try {
            const parsed = JSON.parse(ev.data);
            if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
              writeTerminal(ev.data);
              return;
            }
            const message = parsed as Record<string, unknown>;
            if (message.type === "error" && message.error) {
              writeTerminalLine(`\r\n\x1b[31m${String(message.error)}\x1b[0m`);
              return;
            }
            if (message.type === "connected") {
              const fingerprint =
                typeof message.fingerprint_sha256 === "string"
                  ? ` ${
                      typeof message.host_key_algorithm === "string"
                        ? message.host_key_algorithm
                        : "host-key"
                    } ${message.fingerprint_sha256}`
                : "";
              writeTerminalLine(
                `\x1b[2m— connected to ${
                  typeof message.username === "string" ? message.username : "user"
                }@${typeof message.host === "string" ? message.host : host}${fingerprint} —\x1b[0m`
              );
              return;
            }
          } catch {
            // Not a control message; write it as terminal output.
          }
          // T1-8: Intercept OSC 52 clipboard sequences before writing to terminal
          const osc52 = extractOsc52(ev.data);
          if (osc52) {
            void handleOsc52(osc52.clipboard);
            if (osc52.rest) writeTerminal(osc52.rest);
            return;
          }
          writeTerminal(ev.data);
        } else {
          const uint8 = new Uint8Array(ev.data as ArrayBuffer);
          const text = new TextDecoder().decode(uint8);
          const osc52b = extractOsc52(text);
          if (osc52b) {
            void handleOsc52(osc52b.clipboard);
            if (osc52b.rest) writeTerminal(osc52b.rest);
            return;
          }
          writeTerminal(uint8);
        }
      };
      ws.onerror = () => {
        if (!disposed) writeTerminalLine("\r\n\x1b[31m— connection error —\x1b[0m");
      };
      ws.onclose = () => {
        if (!disposed) writeTerminalLine("\r\n\x1b[33m— disconnected —\x1b[0m");
      };
    })();

    const onResize = () => {
      try {
        fit.fit();
        sendResize();
        highlighter.refresh();
      } catch {
        // ignore transient measure errors
      }
    };
    const observer = new ResizeObserver(onResize);
    observer.observe(container);
    window.addEventListener("resize", onResize);

    return () => {
      disposed = true;
      observer.disconnect();
      window.removeEventListener("resize", onResize);
      window.removeEventListener("agent2ssh:highlights-changed", refreshHighlightRules);
      dataSub.dispose();
      scrollSub.dispose();
      highlighter.dispose();
      ws?.close();
      wsRef.current = null;
      termRef.current = null;
      term.dispose();
    };
  }, [appTheme, host, terminalTheme]);

  return <div ref={containerRef} className="terminal-surface h-full w-full p-2" />;
});

export default TerminalView;
