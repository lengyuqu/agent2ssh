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
import {
  CommandBlockTextLimitError,
  commandBlockMetadata,
  extractCommandBlockText,
  resolveCommandBlockRange,
  type CommandBlockMetadata,
} from "../lib/terminal/block-content";
import {
  createCommandBlockTracker,
  type CommandBlockTracker,
} from "../lib/terminal/command-blocks";
import { createFoldStore, type FoldStore } from "../lib/terminal/folds";
import { renderBlocksToBlob } from "../lib/terminal/block-to-image";

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
  /** Structured block boundaries for future audit correlation. */
  onBlocksChange?: (blocks: CommandBlockMetadata[]) => void;
  onBlockSelected?: (block: CommandBlockMetadata | null) => void;
  /** Live daemon terminal identity used by the authenticated broadcast API. */
  onConnectionChange?: (connection: { terminalId: string; host: string } | null) => void;
};

export type CommandBlockCopyResult =
  | { ok: true; characters: number }
  | { ok: false; reason: "not_found" | "too_large" | "clipboard_unavailable" | "clipboard_failed" };

export type CommandBlockImageResult =
  | { ok: true }
  | { ok: false; reason: "not_found" | "render_failed" | "clipboard_unavailable" | "clipboard_failed" };

export type TerminalViewHandle = {
  /** Inject text as if typed, without a trailing Enter — used by history search
   *  so the user can review/edit a past line before running it. */
  sendText: (text: string) => void;
  getSelection: () => string;
  focus: () => void;
  getBlocks: () => CommandBlockMetadata[];
  searchBlocks: (query: string) => CommandBlockMetadata[];
  selectBlock: (id: string) => boolean;
  copyBlock: (id: string) => Promise<CommandBlockCopyResult>;
  /** Render a block to PNG and write it to the clipboard. */
  copyBlockAsImage: (id: string) => Promise<CommandBlockImageResult>;
  /** Fold/unfold a command block (real buffer splice, not CSS hide). */
  foldBlock: (id: string) => boolean;
  unfoldBlock: (id: string) => boolean;
  isBlockFolded: (id: string) => boolean;
};

/** A live interactive terminal to a host, streamed over the daemon's
 *  /terminal WebSocket (raw bytes both ways: ANSI, control chars, TUIs). */
const TerminalView = forwardRef<TerminalViewHandle, Props>(function TerminalView(
  { host, terminalTheme, appTheme, onLineTyped, onHistoryRequest, onBlocksChange, onBlockSelected, onConnectionChange },
  ref
) {
  const containerRef = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const trackerRef = useRef<CommandBlockTracker | null>(null);
  const foldStoreRef = useRef<FoldStore | null>(null);
  const overlayRef = useRef<HTMLDivElement>(null);
  const blockPaintRef = useRef<{ schedule(): void } | null>(null);
  const selectedBlockIdRef = useRef<string | null>(null);
  const encoderRef = useRef(new TextEncoder());
  const lineBufferRef = useRef("");
  const onLineTypedRef = useRef(onLineTyped);
  onLineTypedRef.current = onLineTyped;
  const onHistoryRequestRef = useRef(onHistoryRequest);
  onHistoryRequestRef.current = onHistoryRequest;
  const onBlocksChangeRef = useRef(onBlocksChange);
  onBlocksChangeRef.current = onBlocksChange;
  const onBlockSelectedRef = useRef(onBlockSelected);
  onBlockSelectedRef.current = onBlockSelected;
  const onConnectionChangeRef = useRef(onConnectionChange);
  onConnectionChangeRef.current = onConnectionChange;

  const metadataFor = (id?: string): CommandBlockMetadata[] => {
    const term = termRef.current;
    const tracker = trackerRef.current;
    if (!term || !tracker) return [];
    const result: CommandBlockMetadata[] = [];
    for (const block of tracker.blocks) {
      if (id && block.id !== id) continue;
      const metadata = commandBlockMetadata(term, host, block);
      if (metadata) result.push(metadata);
    }
    return result;
  };

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
      getSelection: () => termRef.current?.getSelection() ?? "",
      focus: () => termRef.current?.focus(),
      getBlocks: () => metadataFor(),
      searchBlocks: (query: string) => {
        const term = termRef.current;
        const tracker = trackerRef.current;
        if (!term || !tracker) return [];
        const needle = query.trim().toLocaleLowerCase();
        if (!needle) return metadataFor();
        const matches: CommandBlockMetadata[] = [];
        for (const block of tracker.blocks) {
          const commandMatch = block.command?.toLocaleLowerCase().includes(needle) ?? false;
          let outputMatch = false;
          if (!commandMatch) {
            try {
              outputMatch = extractCommandBlockText(term, block).toLocaleLowerCase().includes(needle);
            } catch (error) {
              if (!(error instanceof CommandBlockTextLimitError)) throw error;
            }
          }
          if (commandMatch || outputMatch) {
            const metadata = commandBlockMetadata(term, host, block);
            if (metadata) matches.push(metadata);
          }
        }
        return matches;
      },
      selectBlock: (id: string) => {
        const term = termRef.current;
        const tracker = trackerRef.current;
        const block = tracker?.blocks.find((candidate) => candidate.id === id);
        if (!term || !block || block.start.isDisposed) return false;
        selectedBlockIdRef.current = id;
        term.scrollToLine(Math.max(0, block.start.line - 1));
        blockPaintRef.current?.schedule();
        onBlockSelectedRef.current?.(commandBlockMetadata(term, host, block));
        return true;
      },
      copyBlock: async (id: string) => {
        const term = termRef.current;
        const tracker = trackerRef.current;
        const block = tracker?.blocks.find((candidate) => candidate.id === id);
        if (!term || !block) return { ok: false, reason: "not_found" };
        let text: string;
        try {
          text = extractCommandBlockText(term, block);
        } catch (error) {
          if (error instanceof CommandBlockTextLimitError) return { ok: false, reason: "too_large" };
          return { ok: false, reason: "clipboard_failed" };
        }
        if (!navigator.clipboard?.writeText) return { ok: false, reason: "clipboard_unavailable" };
        try {
          // G4: redact tokens/keys before the copied block reaches the
          // clipboard — same rules as exec/audit/export (copy_redact_rules.json).
          const redacted = await api.redactForClipboard(text);
          await navigator.clipboard.writeText(redacted);
          return { ok: true, characters: text.length };
        } catch {
          return { ok: false, reason: "clipboard_failed" };
        }
      },
      foldBlock: (id: string) => foldStoreRef.current?.fold(id) ?? false,
      unfoldBlock: (id: string) => foldStoreRef.current?.unfold(id) ?? false,
      isBlockFolded: (id: string) => foldStoreRef.current?.isFolded(id) ?? false,
      copyBlockAsImage: async (id: string) => {
        const term = termRef.current;
        const tracker = trackerRef.current;
        const block = tracker?.blocks.find((candidate) => candidate.id === id);
        if (!term || !block) return { ok: false, reason: "not_found" };
        if (!navigator.clipboard?.write || typeof ClipboardItem === "undefined") {
          return { ok: false, reason: "clipboard_unavailable" };
        }
        let blob: Blob | null;
        try {
          blob = await renderBlocksToBlob(term, [block]);
        } catch {
          return { ok: false, reason: "render_failed" };
        }
        if (!blob) return { ok: false, reason: "render_failed" };
        try {
          await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
          return { ok: true };
        } catch {
          return { ok: false, reason: "clipboard_failed" };
        }
      },
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

    const tracker = createCommandBlockTracker(term, {
      idPrefix: `${host}:${Date.now().toString(36)}`,
      getPendingCommand: () => lineBufferRef.current,
    });
    trackerRef.current = tracker;

    const foldStore = createFoldStore(term, tracker);
    foldStoreRef.current = foldStore;

    const paintBlocks = () => {
      const overlay = overlayRef.current;
      const screen = term.element?.querySelector<HTMLElement>(".xterm-screen");
      if (!overlay || !screen || term.buffer.active.type === "alternate") {
        overlay?.replaceChildren();
        return;
      }
      const overlayRect = overlay.getBoundingClientRect();
      const screenRect = screen.getBoundingClientRect();
      const barLeft = Math.max(0, screenRect.left - overlayRect.left - 6);
      const rowHeight = screenRect.height / Math.max(1, term.rows);
      const viewportStart = term.buffer.normal.viewportY;
      const viewportEnd = viewportStart + term.rows - 1;
      const fragment = document.createDocumentFragment();

      for (const block of tracker.blocks) {
        const range = resolveCommandBlockRange(term, block);
        if (!range || range.endLine < viewportStart || range.startLine > viewportEnd) continue;
        const visibleStart = Math.max(range.startLine, viewportStart);
        const visibleEnd = Math.min(range.endLine, viewportEnd);
        const top = screenRect.top - overlayRect.top + (visibleStart - viewportStart) * rowHeight;
        const height = Math.max(3, (visibleEnd - visibleStart + 1) * rowHeight);

        if (selectedBlockIdRef.current === block.id) {
          const selection = document.createElement("div");
          selection.className = "command-block-selection";
          Object.assign(selection.style, {
            left: `${screenRect.left - overlayRect.left}px`,
            top: `${top}px`,
            width: `${screenRect.width}px`,
            height: `${height}px`,
            borderColor: block.color,
          });
          fragment.appendChild(selection);
        }

        const bar = document.createElement("button");
        bar.type = "button";
        bar.className = "command-block-bar";
        bar.dataset.blockId = block.id;
        bar.title = block.command || `Command block ${block.sequence}`;
        bar.setAttribute("aria-label", `Select ${bar.title}`);
        Object.assign(bar.style, {
          left: `${barLeft}px`,
          top: `${top}px`,
          height: `${height}px`,
          backgroundColor: block.color,
        });
        bar.addEventListener("click", () => {
          selectedBlockIdRef.current = block.id;
          blockPaint.schedule();
          onBlockSelectedRef.current?.(commandBlockMetadata(term, host, block));
          term.focus();
        });
        fragment.appendChild(bar);
      }
      overlay.replaceChildren(fragment);
    };
    const blockPaint = createPaintScheduler({ shouldPaint: () => !disposed, paint: paintBlocks });
    blockPaintRef.current = blockPaint;
    const blocksChangedSub = tracker.onChange(() => {
      blockPaint.schedule();
      onBlocksChangeRef.current?.(
        tracker.blocks.flatMap((block) => {
          const metadata = commandBlockMetadata(term, host, block);
          return metadata ? [metadata] : [];
        }),
      );
    });
    const blockWriteSub = term.onWriteParsed(() => blockPaint.schedule());

    const writeTerminal = (data: string | Uint8Array) => {
      term.write(data);
    };
    const writeTerminalLine = (data: string) => {
      term.writeln(data);
    };

    const refreshHighlightRules = () => {
      void api
        .listHighlights()
        .then((rules) => {
          if (disposed) return;
          highlighter.setRules(compileHighlightRules(rules));
        })
        .catch(() => {
          if (disposed) return;
          highlighter.setRules([]);
        });
    };
    refreshHighlightRules();
    window.addEventListener("agent2ssh:highlights-changed", refreshHighlightRules);
    const scrollSub = term.onScroll(() => highlightPaint.schedule());
    const blockScrollSub = term.onScroll(() => blockPaint.schedule());

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
              if (typeof message.terminal_id === "string") {
                onConnectionChangeRef.current?.({ terminalId: message.terminal_id, host });
              }
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
        onConnectionChangeRef.current?.(null);
        if (!disposed) writeTerminalLine("\r\n\x1b[33m— disconnected —\x1b[0m");
      };
    })();

    const onResize = () => {
      try {
        // Folds are saved at the old column width — expand them before the
        // terminal reflows its rows.
        foldStoreRef.current?.unfoldAll();
        fit.fit();
        sendResize();
        highlightPaint.schedule();
        blockPaint.schedule();
      } catch {
        // ignore transient measure errors
      }
    };
    const observer = new ResizeObserver(onResize);
    observer.observe(container);
    window.addEventListener("resize", onResize);

    return () => {
      disposed = true;
      onConnectionChangeRef.current?.(null);
      observer.disconnect();
      window.removeEventListener("resize", onResize);
      window.removeEventListener("agent2ssh:highlights-changed", refreshHighlightRules);
      dataSub.dispose();
      scrollSub.dispose();
      blockScrollSub.dispose();
      writeParsedSub.dispose();
      blockWriteSub.dispose();
      clipboardOscSub.dispose();
      blocksChangedSub.dispose();
      blockPaint.dispose();
      blockPaintRef.current = null;
      foldStoreRef.current?.dispose();
      foldStoreRef.current = null;
      tracker.dispose();
      trackerRef.current = null;
      overlayRef.current?.replaceChildren();
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
  }, [host]);

  // Theme changes repaint the terminal instead of rebuilding it — rebuilding
  // would destroy the session (WebSocket), scrollback, folds and command blocks.
  useEffect(() => {
    const term = termRef.current;
    if (term) {
      term.options.theme = resolveTerminalTheme(terminalTheme, appTheme);
    }
  }, [appTheme, terminalTheme]);

  return (
    <div className="terminal-surface relative h-full w-full overflow-hidden p-2">
      <div ref={containerRef} className="h-full w-full" />
      <div ref={overlayRef} className="pointer-events-none absolute inset-0 z-[3] overflow-hidden" />
    </div>
  );
});

export default TerminalView;
