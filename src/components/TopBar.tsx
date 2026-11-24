import { memo, useEffect, useRef, useState } from "react";
import { Lock, LockOpen, Radio, ShieldAlert, Unplug } from "lucide-react";
import type { ReactNode } from "react";
import { useI18n } from "../i18n";
import { cn } from "../lib/utils";
import type { ConnectionStatus, DaemonHealth, ExecutionGateStatus } from "../types";
import { version as appVersion } from "../../package.json";

type TopBarProps = {
  /** Status props migrated verbatim from the retired bottom status bar. */
  daemonHealth: DaemonHealth | null;
  gateStatus: ExecutionGateStatus | null;
  gateBusy?: boolean;
  secretsLocked: boolean;
  connectionStatuses: ConnectionStatus[];
  pendingApprovalsCount: number;
  onOpenPalette: () => void;
  onOpenApprovals: () => void;
  onStartDaemon?: () => void | Promise<void>;
  onGateToggle?: () => void;
  onRequestUnlock?: () => void;
  onNavigateModule?: (id: string) => void;
  /** True while an SSH connect is in flight. Pulses the connection pill. */
  connectionBusy?: boolean;
  /** Right-end slot (SettingsMenu keeps its own props). */
  actions?: ReactNode;
};

const pillBtnCls =
  "rounded px-1.5 py-0.5 transition-colors hover:bg-canvas-2 hover:text-foreground disabled:cursor-not-allowed disabled:opacity-60";

/** v3 3.1 status top bar: A2 mark + agent pills (daemon / gate / credentials /
 * active connections, migrated from the bottom status bar) on canvas-0,
 * version + ⌘K entry on the right. Single 36px row; low-priority pills hide
 * below lg so narrow windows (<1024px) never wrap. */
function TopBar({
  daemonHealth,
  gateStatus,
  gateBusy = false,
  secretsLocked,
  connectionStatuses,
  pendingApprovalsCount,
  onOpenPalette,
  onOpenApprovals,
  onStartDaemon,
  onGateToggle,
  onRequestUnlock,
  onNavigateModule,
  connectionBusy = false,
  actions,
}: TopBarProps) {
  const { t } = useI18n();
  const [daemonStarting, setDaemonStarting] = useState(false);
  const startingRef = useRef(false);
  const aliveRef = useRef(true);
  const daemonOk = daemonHealth?.ok === true;
  const gatePaused = gateStatus?.mode === "paused";
  const activeConnections = connectionStatuses.filter((c) => c.connected).length;

  useEffect(() => {
    aliveRef.current = true;
    return () => {
      aliveRef.current = false;
    };
  }, []);

  function handleDaemonClick() {
    if (daemonOk) {
      onNavigateModule?.("activity");
      return;
    }
    if (!onStartDaemon || startingRef.current) return;
    startingRef.current = true;
    setDaemonStarting(true);
    void Promise.resolve(onStartDaemon()).finally(() => {
      startingRef.current = false;
      if (aliveRef.current) setDaemonStarting(false);
    });
  }

  return (
    <header className="flex h-9 shrink-0 items-center gap-3 border-b border-line bg-canvas-0 px-3 text-xs text-muted-foreground">
      {/* Brand mark (neutral — cyan is reserved for clickable actions) */}
      <span className="flex shrink-0 items-center gap-2">
        <span className="rounded border border-edge px-1.5 py-0.5 font-mono text-[11px] font-bold text-foreground">
          A2
        </span>
        <span className="hidden font-semibold text-foreground md:inline">Agent2SSH</span>
      </span>

      {/* Agent status pills */}
      <div className="flex min-w-0 items-center gap-1.5 overflow-hidden">
        <button
          type="button"
          onClick={handleDaemonClick}
          disabled={!daemonOk && (!onStartDaemon || daemonStarting)}
          aria-busy={daemonStarting || undefined}
          className={cn("flex shrink-0 items-center gap-1.5", pillBtnCls)}
          title={daemonOk ? t("Local daemon health") : t("Start daemon")}
        >
          <span
            className={cn(
              "size-1.5 rounded-full",
              daemonOk ? "bg-success" : daemonStarting ? "animate-pulse bg-warning" : "bg-destructive"
            )}
            aria-hidden
          />
          {daemonOk
            ? t("Daemon running")
            : daemonStarting
              ? t("Starting...")
              : t("Daemon offline")}
        </button>

        <button
          type="button"
          onClick={() => onGateToggle?.()}
          disabled={gateBusy || gateStatus === null || !onGateToggle}
          aria-busy={gateBusy || undefined}
          className={cn("hidden shrink-0 items-center gap-1.5 sm:flex", pillBtnCls)}
          title={
            gateStatus === null
              ? t("Execution gate")
              : gatePaused
                ? t("Resume gate")
                : t("Pause gate")
          }
        >
          {gatePaused ? (
            <ShieldAlert size={11} className="text-risk-high" />
          ) : (
            <Radio size={11} className={gateStatus ? "text-success" : undefined} />
          )}
          {gateStatus === null
            ? t("Gate unavailable")
            : gatePaused
              ? t("Gate paused")
              : t("Gate active")}
        </button>

        <button
          type="button"
          onClick={() => {
            if (secretsLocked) onRequestUnlock?.();
            else onNavigateModule?.("keys");
          }}
          disabled={secretsLocked ? !onRequestUnlock : !onNavigateModule}
          className={cn("hidden shrink-0 items-center gap-1.5 md:flex", pillBtnCls)}
          title={secretsLocked ? t("Unlock credentials") : t("Credential store")}
        >
          {secretsLocked ? <Lock size={11} /> : <LockOpen size={11} />}
          {secretsLocked ? t("Credentials locked") : t("Credentials unlocked")}
        </button>

        <button
          type="button"
          onClick={() => onNavigateModule?.("hosts")}
          disabled={!onNavigateModule}
          className={cn(
            "hidden shrink-0 items-center gap-1.5 lg:flex",
            pillBtnCls,
            connectionBusy && "animate-pulse text-primary"
          )}
          aria-busy={connectionBusy || undefined}
          title={t("Active embedded SSH connections")}
        >
          <Unplug size={11} />
          {t("{count} active connections", { count: activeConnections })}
        </button>

        {pendingApprovalsCount > 0 && (
          <button
            type="button"
            onClick={() => onOpenApprovals()}
            className="flex shrink-0 items-center gap-1.5 rounded border border-risk-medium/40 bg-risk-medium/10 px-1.5 py-0.5 font-medium text-risk-medium hover:border-risk-medium/70"
            title={t("Pending approvals")}
          >
            <span className="size-1.5 animate-pulse rounded-full bg-risk-medium" aria-hidden />
            {pendingApprovalsCount}{" "}
            {t(pendingApprovalsCount > 1 ? "pending approvals" : "pending approval")}
          </button>
        )}
      </div>

      {/* Version + ⌘K entry + settings slot */}
      <div className="ml-auto flex shrink-0 items-center gap-2">
        <span className="hidden font-mono text-[11px] text-faint sm:inline">v{appVersion}</span>
        <button
          type="button"
          onClick={onOpenPalette}
          className="rounded-md border border-edge px-2 py-0.5 font-mono text-[11px] text-primary transition-colors hover:border-edge-strong hover:bg-canvas-2"
          title={t("Open command palette")}
        >
          ⌘K
        </button>
        {actions}
      </div>
    </header>
  );
}

export default memo(TopBar);
