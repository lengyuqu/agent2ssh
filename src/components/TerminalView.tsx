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
import { registerClipboardOscHandler } from "../lib/terminal/osc52";
import { createPaintScheduler } from "../lib/terminal/paint-scheduler";

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

    const highlightPaint = createPaintScheduler({
      shouldPaint: () => !disposed,
      paint: () => highlighter.refresh(),
    });
    const writeParsedSub = term.onWriteParsed(() => highlightPaint.schedule());
    const clipboardOscSub = registerClipboardOscHandler(term.parser, {
      writeText: (text) => navigator.clipboard?.writeText(text),
    });

    const writeTerminal = (data: string | Uint8Array) => {
      term.write(data);
    };
    const writeTerminalLine = (data: string) => {
      term.writeln(data);
    };

    const refreshHighlightRules = () => {
      void api
        .listHighlights()
        .then((rules) => highlighter.setRules(compileHighlightRules(rules)))
        .catch(() => highlighter.setRules([]));
    };
    refreshHighlightRules();
    window.addEventListener("agent2ssh:highlights-changed", refreshHighlightRules);
    const scrollSub = term.onScroll(() => highlightPaint.schedule());

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
        if (!disposed) {
          writeTerminalLine(`\x1b[31mFailed to start daemon: ${String(err)}\x1b[0m`);
        }
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
          writeTerminal(ev.data);
        } else {
          const uint8 = new Uint8Array(ev.data as ArrayBuffer);
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
        highlightPaint.schedule();
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
      writeParsedSub.dispose();
      clipboardOscSub.dispose();
      highlightPaint.dispose();
      highlighter.dispose();
      if (ws) {
        ws.onopen = null;
        ws.onmessage = null;
        ws.onerror = null;
        ws.onclose = null;
        ws.close();
      }
      wsRef.current = null;
      termRef.current = null;
      term.dispose();
    };
  }, [appTheme, host, terminalTheme]);

  return <div ref={containerRef} className="terminal-surface h-full w-full p-2" />;
});

export default TerminalView;
