import { Maximize2, X } from "lucide-react";
import { memo, useEffect, useRef } from "react";
import { TerminalSlot } from "../features/terminal/dock";
import { useI18n } from "../i18n";
import { cn } from "../lib/utils";
import { Button } from "./ui/button";
import { IconButton } from "./ui/icon-button";

type Props = {
  open: boolean;
  host: string;
  /** Command the docked view replays once. `null` does not type anything —
   *  the approval path already ran on the daemon. */
  command: string | null;
  /** The live view is portaled into this slot so it can move to the main terminal. */
  slotId: string | null;
  connected: boolean;
  onClose: () => void;
  /** Move the same PTY into the main Terminal module. */
  onOpenInTerminal: (host: string) => void;
};

/** v3 §3.4: right-side terminal drawer. The PTY itself lives in `LiveTerminal`
 *  and is only shown here while the slot is the drawer. Esc or the overlay
 *  dismisses it and drops that session. */
function TerminalDrawer({ open, host, command, slotId, connected, onClose, onOpenInTerminal }: Props) {
  const { t } = useI18n();
  const rootRef = useRef<HTMLDivElement>(null);

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

  return (
    <div ref={rootRef} className="fixed inset-0 z-[1100]">
      {/* Overlay click dismisses; the drawer sits above it on canvas-0. */}
      <div className="absolute inset-0 bg-black/50" onClick={onClose} aria-hidden />
      <aside className="absolute inset-y-0 right-0 flex w-[480px] max-w-full flex-col border-l border-line bg-canvas-0">
        <div className="flex shrink-0 items-center gap-2 border-b border-line px-3 py-2">
          <span
            className={cn(
              "size-1.5 shrink-0 rounded-full",
              connected ? "bg-risk-low" : "bg-muted-foreground/50"
            )}
            aria-hidden
          />
          <span className="truncate font-mono text-sm font-semibold text-foreground">{host}</span>
          <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-faint">
            {command
              ? t("Replay")
              : connected
                ? t("Approved · daemon executing on this host")
                : t("Live terminal")}
          </span>
          <Button
            size="sm"
            variant="outline"
            className="h-7 shrink-0 px-2 text-xs font-medium"
            title={t("Open in Terminal")}
            onClick={() => onOpenInTerminal(host)}
          >
            <Maximize2 size={13} />
            {t("Open in Terminal")}
          </Button>
          <IconButton onClick={onClose} title={t("Close")}>
            <X size={15} />
          </IconButton>
        </div>
        <div className="min-h-0 flex-1">
          {slotId && <TerminalSlot id={slotId} className="h-full" />}
        </div>
      </aside>
    </div>
  );
}

export default memo(TerminalDrawer);
