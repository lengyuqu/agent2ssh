import { memo, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { cn } from "../lib/utils";
import type { HostProfile } from "../types";
import ExecPanel from "./ExecPanel";
import MultiExecPanel from "./MultiExecPanel";
import PlaybooksPanel from "./PlaybooksPanel";

export const EXEC_MODE_STORAGE_KEY = "agent2ssh.execMode";

export type ExecMode = "single" | "multi" | "playbooks";

const MODES: Array<{ id: ExecMode; label: string }> = [
  { id: "single", label: "Single Host" },
  { id: "multi", label: "Multi-Host" },
  { id: "playbooks", label: "Playbooks" },
];

type Props = {
  hosts: HostProfile[];
  initialHost?: string;
  initialCommand?: string;
  /** Bumps when the same command is carried in again so the draft is replaced. */
  carryEpoch?: number;
  /**
   * One-shot mode from outside (approval jump → `"single"`, the playbooks
   * module → `"playbooks"`). Applied over the stored mode, then reported via
   * `onPreferredModeApplied` so a later visit can restore localStorage.
   */
  preferredMode?: ExecMode;
  onPreferredModeApplied?: () => void;
  /** Fired when the user picks a tab. Programmatic mode changes stay local. */
  onModeChange?: (mode: ExecMode) => void;
  onExecComplete: () => void;
};

function readExecMode(): ExecMode {
  try {
    const saved = localStorage.getItem(EXEC_MODE_STORAGE_KEY);
    if (saved === "single" || saved === "multi" || saved === "playbooks") return saved;
  } catch {
    // Private mode and opaque origins throw; the in-memory default still works.
  }
  return "single";
}

/** Single host, multi-host, and playbooks as one segmented surface. */
function ExecutionWorkbench({
  hosts,
  initialHost,
  initialCommand,
  carryEpoch = 0,
  preferredMode,
  onPreferredModeApplied,
  onModeChange,
  onExecComplete,
}: Props) {
  const { t } = useI18n();
  const [mode, setMode] = useState<ExecMode>(() => preferredMode ?? readExecMode());
  // Left unset so the first layout pass still reports `preferredMode` (the
  // caller drops the one-shot). A later visit with no preferredMode restores
  // localStorage via the state initializer above.
  const preferredApplied = useRef<ExecMode | undefined>(undefined);
  const seenCommand = useRef(initialCommand);
  const seenEpoch = useRef(carryEpoch);

  useLayoutEffect(() => {
    if (!preferredMode) {
      preferredApplied.current = undefined;
      return;
    }
    if (preferredApplied.current === preferredMode) return;
    preferredApplied.current = preferredMode;
    setMode(preferredMode);
    onPreferredModeApplied?.();
  }, [preferredMode, onPreferredModeApplied]);

  useLayoutEffect(() => {
    const commandArrived =
      Boolean(initialCommand?.trim()) && seenCommand.current !== initialCommand;
    const epochArrived = seenEpoch.current !== carryEpoch;
    seenCommand.current = initialCommand;
    seenEpoch.current = carryEpoch;
    if (!commandArrived && !epochArrived) return;
    // The playbooks module pins preferredMode. An approval jump leaves that
    // route, so this instance only yields when the outside ask is single
    // (or absent, meaning the command itself is the ask).
    if (preferredMode && preferredMode !== "single") return;
    setMode("single");
  }, [initialCommand, carryEpoch, preferredMode]);

  useEffect(() => {
    try {
      localStorage.setItem(EXEC_MODE_STORAGE_KEY, mode);
    } catch {
      // The selected tab still works when storage is blocked.
    }
  }, [mode]);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4">
      <div
        role="tablist"
        aria-label={t("Execute")}
        className="inline-flex max-w-full flex-wrap items-center gap-1 rounded-full border border-border bg-card p-1"
      >
        {MODES.map((item) => {
          const selected = mode === item.id;
          return (
            <button
              key={item.id}
              type="button"
              role="tab"
              aria-selected={selected}
              className={cn(
                "rounded-full px-3 py-1.5 text-sm font-semibold transition-colors",
                selected
                  ? "bg-primary text-primary-foreground"
                  : "text-muted-foreground hover:text-foreground",
              )}
              onClick={() => {
                setMode(item.id);
                onModeChange?.(item.id);
              }}
            >
              {t(item.label)}
            </button>
          );
        })}
      </div>
      <div role="tabpanel" className="flex min-h-0 min-w-0 flex-1 flex-col">
        {mode === "single" && (
          <ExecPanel
            hosts={hosts}
            initialHost={initialHost}
            initialCommand={initialCommand}
            carryEpoch={carryEpoch}
            onExecComplete={onExecComplete}
          />
        )}
        {mode === "multi" && <MultiExecPanel hosts={hosts} onExecComplete={onExecComplete} />}
        {mode === "playbooks" && <PlaybooksPanel hosts={hosts} initialHost={initialHost} />}
      </div>
    </div>
  );
}

export default memo(ExecutionWorkbench);
