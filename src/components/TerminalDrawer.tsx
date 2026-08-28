import { X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import {
  isTerminalThemeId,
  TERMINAL_THEME_STORAGE_KEY,
  type TerminalThemeId,
} from "../terminalThemes";
import { useI18n } from "../i18n";
import { useTheme, type Theme as AppTheme } from "../theme";
import { cn } from "../lib/utils";
import TerminalView, { type TerminalViewHandle } from "./TerminalView";
import { IconButton } from "./ui/icon-button";

type Props = {
  open: boolean;
  host: string;
  /** Command to replay once the drawer's PTY session is ready (typed into the
   *  shell with Enter). `null` opens the live session without typing anything —
   *  used by the approval path: the daemon executes the approved command itself,
   *  so injecting it here would run it a second time. */
  command: string | null;
  onClose: () => void;
};

function initialTerminalTheme(): TerminalThemeId {
  try {
    const saved = localStorage.getItem(TERMINAL_THEME_STORAGE_KEY);
    if (saved && isTerminalThemeId(saved)) return saved;
  } catch {
    // localStorage may be unavailable
  }
  return "app";
}

/** Short delay between the daemon "connected" handshake and the shell prompt
 *  actually accepting input, so a replayed command isn't swallowed by the PTY. */
const REPLAY_DELAY_MS = 400;

/** v3 §3.4: right-side terminal drawer (canvas-0, ~480px) hosting a live
 *  TerminalView. Session model: the drawer mounts its own TerminalView, which
 *  opens its own daemon /terminal WebSocket — the same per-view session model
 *  the Terminal module already uses, so no backend changes and no risk to
 *  existing terminal tabs. Raised from the approval workbench ("approve and
 *  execute") and audit rows ("replay"); Esc or the overlay dismisses it. */
export default function TerminalDrawer({ open, host, command, onClose }: Props) {
  const { t } = useI18n();
  const { theme: appTheme } = useTheme();
  const [terminalTheme] = useState<TerminalThemeId>(initialTerminalTheme);
  const [connection, setConnection] = useState<{ terminalId: string; host: string } | null>(null);
  const viewRef = useRef<TerminalViewHandle>(null);
  // Replay bookkeeping: the command survives the connect handshake, then is
  // typed once. Cleared if the drawer closes or the command is consumed.
  const pendingCommandRef = useRef<string | null>(null);
  const replayTimerRef = useRef<number | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  // Arm the replay whenever the drawer opens with a command to run.
  useEffect(() => {
    if (!open) {
      pendingCommandRef.current = null;
      if (replayTimerRef.current !== null) {
        window.clearTimeout(replayTimerRef.current);
        replayTimerRef.current = null;
      }
      return;
    }
    pendingCommandRef.current = command;
  }, [open, command]);

  // Esc closes the drawer — except when the terminal itself has focus, where
  // Esc is a real key for the remote shell (vim, less, …).
  useEffect(() => {
    if (!open) return;
    function onKeyDown(e: KeyboardEvent) {
      if (e.key !== "Escape") return;
      const target = e.target as HTMLElement | null;
      if (target && rootRef.current?.contains(target)) return;
      e.preventDefault();
      onClose();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  if (!open || !host) return null;

  function handleConnectionChange(next: { terminalId: string; host: string } | null) {
    setConnection(next);
    const commandToRun = pendingCommandRef.current;
    if (next && commandToRun) {
      pendingCommandRef.current = null;
      if (replayTimerRef.current !== null) window.clearTimeout(replayTimerRef.current);
      replayTimerRef.current = window.setTimeout(() => {
        viewRef.current?.sendText(`${commandToRun}\r`);
        viewRef.current?.focus();
      }, REPLAY_DELAY_MS);
    }
  }

  return (
    <div ref={rootRef} className="fixed inset-0 z-[1100]">
      {/* Overlay click dismisses; the drawer sits above it on canvas-0. */}
      <div className="absolute inset-0 bg-black/50" onClick={onClose} aria-hidden />
      <aside className="absolute inset-y-0 right-0 flex w-[480px] max-w-full flex-col border-l border-line bg-canvas-0">
        <div className="flex shrink-0 items-center gap-2 border-b border-line px-3 py-2">
          <span
            className={cn(
              "size-1.5 shrink-0 rounded-full",
              connection ? "bg-risk-low" : "bg-muted-foreground/50"
            )}
            aria-hidden
          />
          <span className="truncate font-mono text-sm font-semibold text-foreground">{host}</span>
          <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-faint">
            {command
              ? t("Replay")
              : connection
                ? t("Approved · daemon executing on this host")
                : t("Live terminal")}
          </span>
          <IconButton onClick={onClose} title={t("Close")}>
            <X size={15} />
          </IconButton>
        </div>
        <div className="min-h-0 flex-1">
          <TerminalView
            ref={viewRef}
            key={host}
            host={host}
            terminalTheme={terminalTheme}
            appTheme={appTheme}
            onConnectionChange={handleConnectionChange}
          />
        </div>
      </aside>
    </div>
  );
}
