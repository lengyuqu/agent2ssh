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
  secretsLocked: boolean;
  connectionStatuses: ConnectionStatus[];
  pendingApprovalsCount: number;
  onOpenPalette: () => void;
  onOpenApprovals: () => void;
  /** Right-end slot (SettingsMenu keeps its own props). */
  actions?: ReactNode;
};

/** v3 3.1 status top bar: A2 mark + agent pills (daemon / gate / credentials /
 * active connections, migrated from the bottom status bar) on canvas-0,
 * version + ⌘K entry on the right. Single 36px row; low-priority pills hide
 * below lg so narrow windows (<1024px) never wrap. */
export default function TopBar({
  daemonHealth,
  gateStatus,
  secretsLocked,
  connectionStatuses,
  pendingApprovalsCount,
  onOpenPalette,
  onOpenApprovals,
  actions,
}: TopBarProps) {
  const { t } = useI18n();
  const daemonOk = daemonHealth?.ok === true;
  const gatePaused = gateStatus?.mode === "paused";
  const activeConnections = connectionStatuses.filter((c) => c.connected).length;

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
      <div className="flex min-w-0 items-center gap-3 overflow-hidden">
        <span className="flex shrink-0 items-center gap-1.5" title={t("Local daemon health")}>
          <span
            className={cn("size-1.5 rounded-full", daemonOk ? "bg-success" : "bg-destructive")}
            aria-hidden
          />
          {daemonOk ? t("Daemon running") : t("Daemon offline")}
        </span>

        <span
          className="hidden shrink-0 items-center gap-1.5 sm:flex"
          title={t("Execution gate")}
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
        </span>

        <span
          className="hidden shrink-0 items-center gap-1.5 md:flex"
          title={t("Credential store")}
        >
          {secretsLocked ? <Lock size={11} /> : <LockOpen size={11} />}
          {secretsLocked ? t("Credentials locked") : t("Credentials unlocked")}
        </span>

        <span
          className="hidden shrink-0 items-center gap-1.5 lg:flex"
          title={t("Active embedded SSH connections")}
        >
          <Unplug size={11} />
          {t("{count} active connections", { count: activeConnections })}
        </span>

        {pendingApprovalsCount > 0 && (
          <button
            type="button"
            onClick={onOpenApprovals}
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
