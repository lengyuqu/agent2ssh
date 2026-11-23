import { useEffect, useRef, useState } from "react";
import { useTheme, type Theme as AppTheme } from "../../theme";
import {
  isTerminalThemeId,
  TERMINAL_THEME_STORAGE_KEY,
  type TerminalThemeId,
} from "../../terminalThemes";
import TerminalView, { type TerminalViewHandle } from "../../components/TerminalView";

type Props = {
  host: string;
  /** Typed once after connect. `null` leaves the shell alone (approval path). */
  command: string | null;
  onConnectedChange?: (connected: boolean) => void;
};

const REPLAY_DELAY_MS = 400;

function initialTerminalTheme(): TerminalThemeId {
  try {
    const saved = localStorage.getItem(TERMINAL_THEME_STORAGE_KEY);
    if (saved && isTerminalThemeId(saved)) return saved;
  } catch {
    // localStorage may be unavailable
  }
  return "app";
}

/** The drawer's PTY. Stays mounted when the view is moved into the main terminal. */
export default function LiveTerminal({ host, command, onConnectedChange }: Props) {
  const { theme: appTheme } = useTheme();
  const [terminalTheme] = useState<TerminalThemeId>(initialTerminalTheme);
  const viewRef = useRef<TerminalViewHandle>(null);
  const pendingCommandRef = useRef<string | null>(command);
  const replayTimerRef = useRef<number | null>(null);
  const onConnectedChangeRef = useRef(onConnectedChange);
  onConnectedChangeRef.current = onConnectedChange;

  useEffect(() => {
    return () => {
      if (replayTimerRef.current !== null) window.clearTimeout(replayTimerRef.current);
    };
  }, []);

  function handleConnectionChange(next: { terminalId: string; host: string } | null) {
    onConnectedChangeRef.current?.(next !== null);
    const commandToRun = pendingCommandRef.current;
    if (!next || !commandToRun) return;
    pendingCommandRef.current = null;
    if (replayTimerRef.current !== null) window.clearTimeout(replayTimerRef.current);
    replayTimerRef.current = window.setTimeout(() => {
      viewRef.current?.sendText(`${commandToRun}\r`);
      viewRef.current?.focus();
    }, REPLAY_DELAY_MS);
  }

  return (
    <TerminalView
      ref={viewRef}
      host={host}
      terminalTheme={terminalTheme}
      appTheme={appTheme as AppTheme}
      onConnectionChange={handleConnectionChange}
    />
  );
}
