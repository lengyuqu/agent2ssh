import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { api, getDaemonUrl } from "../api";
import type { TerminalThemeId } from "../terminalThemes";
import { resolveTerminalTheme } from "../terminalThemes";
import type { Theme as AppTheme } from "../theme";

type Props = {
  host: string;
  terminalTheme: TerminalThemeId;
  appTheme: AppTheme;
};

/** A live interactive terminal to a host, streamed over the daemon's
 *  /terminal WebSocket (raw bytes both ways: ANSI, control chars, TUIs). */
export default function TerminalView({ host, terminalTheme, appTheme }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);

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
    const encoder = new TextEncoder();

    function sendResize() {
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: "resize", cols: term.cols, rows: term.rows }));
      }
    }

    // Keyboard / paste → PTY (as binary frames).
    const dataSub = term.onData((data) => {
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(encoder.encode(data));
      }
    });

    (async () => {
      let token = "";
      try {
        await api.daemonStart();
        token = await api.getDaemonToken();
      } catch (err) {
        term.writeln(`\x1b[31mFailed to start daemon: ${String(err)}\x1b[0m`);
        return;
      }
      if (disposed) return;
      const base = getDaemonUrl().replace(/^http/, "ws");
      const url = `${base}/terminal?host=${encodeURIComponent(host)}&token=${encodeURIComponent(token)}`;
      ws = new WebSocket(url);
      ws.binaryType = "arraybuffer";
      ws.onopen = () => {
        sendResize();
      };
      ws.onmessage = (ev) => {
        if (typeof ev.data === "string") {
          try {
            const parsed = JSON.parse(ev.data);
            if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
              term.write(ev.data);
              return;
            }
            const message = parsed as Record<string, unknown>;
            if (message.type === "error" && message.error) {
              term.writeln(`\r\n\x1b[31m${String(message.error)}\x1b[0m`);
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
              term.writeln(
                `\x1b[2m— connected to ${
                  typeof message.username === "string" ? message.username : "user"
                }@${typeof message.host === "string" ? message.host : host}${fingerprint} —\x1b[0m`
              );
              return;
            }
          } catch {
            // Not a control message; write it as terminal output.
          }
          term.write(ev.data);
        } else {
          term.write(new Uint8Array(ev.data as ArrayBuffer));
        }
      };
      ws.onerror = () => {
        if (!disposed) term.writeln("\r\n\x1b[31m— connection error —\x1b[0m");
      };
      ws.onclose = () => {
        if (!disposed) term.writeln("\r\n\x1b[33m— disconnected —\x1b[0m");
      };
    })();

    const onResize = () => {
      try {
        fit.fit();
        sendResize();
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
      dataSub.dispose();
      ws?.close();
      term.dispose();
    };
  }, [appTheme, host, terminalTheme]);

  return <div ref={containerRef} className="terminal-surface h-full w-full p-2" />;
}
