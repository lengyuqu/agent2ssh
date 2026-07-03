import { Lock, LockOpen, Radio, ShieldAlert, Unplug } from "lucide-react";
import { useI18n } from "../i18n";
import { cn } from "../lib/utils";
import type { ConnectionStatus, DaemonHealth, ExecutionGateStatus } from "../types";
import { version as appVersion } from "../../package.json";

type FootbarProps = {
  daemonHealth: DaemonHealth | null;
  gateStatus: ExecutionGateStatus | null;
  secretsLocked: boolean;
  connectionStatuses: ConnectionStatus[];
};

/** V1-2: fixed status bar — daemon, gate, credential lock, active connections, version. */
export default function Footbar({
  daemonHealth,
  gateStatus,
  secretsLocked,
  connectionStatuses,
}: FootbarProps) {
  const { t } = useI18n();
  const daemonOk = daemonHealth?.ok === true;
  const gatePaused = gateStatus?.mode === "paused";
  const activeConnections = connectionStatuses.filter((c) => c.connected).length;

  return (
    <footer className="flex h-8 shrink-0 items-center gap-4 border-t border-border bg-card px-4 text-xs text-muted-foreground">
      <span className="flex items-center gap-1.5" title={t("Local daemon health")}>
        <span
          className={cn("size-1.5 rounded-full", daemonOk ? "bg-success" : "bg-destructive")}
          aria-hidden
        />
        {daemonOk ? t("Daemon running") : t("Daemon offline")}
      </span>

      <span className="flex items-center gap-1.5" title={t("Execution gate")}>
        {gatePaused ? (
          <ShieldAlert size={11} className="text-destructive" />
        ) : (
          <Radio size={11} className={gateStatus ? "text-success" : "text-muted-foreground"} />
        )}
        {gateStatus === null
          ? t("Gate unavailable")
          : gatePaused
            ? t("Gate paused")
            : t("Gate active")}
      </span>

      <span className="flex items-center gap-1.5" title={t("Credential store")}>
        {secretsLocked ? <Lock size={11} /> : <LockOpen size={11} />}
        {secretsLocked ? t("Credentials locked") : t("Credentials unlocked")}
      </span>

      <span className="flex items-center gap-1.5" title={t("Active embedded SSH connections")}>
        <Unplug size={11} />
        {t("{count} active connections", { count: activeConnections })}
      </span>

      <span className="ml-auto font-mono text-[11px] text-muted-foreground/70">v{appVersion}</span>
    </footer>
  );
}
