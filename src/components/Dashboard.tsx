import {
  AlertTriangle,
  CheckCircle2,
  Lock,
  LockOpen,
  RadioTower,
  Server,
  ShieldAlert,
  XCircle,
} from "lucide-react";
import { memo, useEffect, useRef, useState  } from "react";
import { api } from "../api";
import { useAgentEvents, useEventsStatus } from "../eventsBus";
import { useI18n } from "../i18n";
import { cn } from "../lib/utils";
import type { ConnectionStatus, DaemonHealth, HostProfile } from "../types";
import { Card, CardContent, CardHeader, CardTitle } from "./ui/card";
import PingPanel from "./PingPanel";

const EXEC_COUNT_REFRESH_MS = 60_000;

type DashboardProps = {
  hosts: HostProfile[];
  connectionStatuses: ConnectionStatus[];
  pendingApprovalsCount: number;
  secretsLocked: boolean;
  daemonHealth: DaemonHealth | null;
  onNavigateModule: (id: string) => void;
  onRequestUnlock?: () => void;
  onStartDaemon?: () => void | Promise<void>;
  onOpenAnomalies?: () => void;
  onOpenHostTerminal?: (host: string) => void;
  onInspectHost?: (host: string) => void;
};

/** Loading, a counted window, or a failed read. One null used to mean all three. */
type ExecCountState =
  | { status: "loading" }
  | { status: "ready"; count: number }
  | { status: "error" };

type StatCardProps = {
  icon: React.ReactNode;
  label: string;
  value: React.ReactNode;
  hint?: string;
  tone?: "default" | "success" | "warning" | "destructive";
  busy?: boolean;
  onClick?: () => void;
};

function StatCard({ icon, label, value, hint, tone = "default", busy = false, onClick }: StatCardProps) {
  return (
    <Card
      className={cn(
        "transition-colors",
        onClick && "cursor-pointer hover:border-ring/50",
        tone === "success" && "border-success/30",
        tone === "warning" && "border-warning/30",
        tone === "destructive" && "border-destructive/30"
      )}
      onClick={onClick}
      role={onClick ? "button" : undefined}
      aria-busy={busy || undefined}
    >
      <CardHeader className="pb-2">
        <CardTitle className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
          <span
            className={cn(
              "flex size-6 items-center justify-center rounded-md",
              tone === "success" && "bg-success/15 text-success",
              tone === "warning" && "bg-warning/15 text-warning",
              tone === "destructive" && "bg-destructive/15 text-destructive",
              tone === "default" && "bg-muted text-muted-foreground"
            )}
          >
            {icon}
          </span>
          {label}
        </CardTitle>
      </CardHeader>
      <CardContent className="flex items-baseline gap-2 pt-0">
        <span className="text-2xl font-bold text-foreground">{value}</span>
        {hint && <span className="text-xs text-muted-foreground">{hint}</span>}
      </CardContent>
    </Card>
  );
}

/** V1-1: landing dashboard aggregating host health, approvals, anomalies, 24h volume, credentials, daemon. */
function Dashboard({
  hosts,
  connectionStatuses,
  pendingApprovalsCount,
  secretsLocked,
  daemonHealth,
  onNavigateModule,
  onRequestUnlock,
  onStartDaemon,
  onOpenAnomalies,
  onOpenHostTerminal,
  onInspectHost,
}: DashboardProps) {
  const { t } = useI18n();
  const [execCount, setExecCount] = useState<ExecCountState>({ status: "loading" });
  const [daemonStarting, setDaemonStarting] = useState(false);
  const daemonStartRef = useRef(false);
  const aliveRef = useRef(true);
  const [anomalyCount, setAnomalyCount] = useState(0);
  // V2-1: shared event bus (single SSE connection) instead of a dashboard-local
  // subscription — LiveActivityPanel and NotificationCenter use the same one.
  const anomalyStreamStatus = useEventsStatus();

  useAgentEvents((event) => {
    if (event.event_type === "anomaly_detected") {
      setAnomalyCount((count) => count + 1);
    }
  });

  const daemonOk = daemonHealth?.ok === true;

  useEffect(() => {
    aliveRef.current = true;
    return () => {
      aliveRef.current = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    async function loadExecCount() {
      try {
        const since = new Date(Date.now() - 24 * 60 * 60 * 1000).toISOString();
        const entries = await api.listAudit({ since, limit: 5000 });
        if (active) setExecCount({ status: "ready", count: entries.length });
      } catch {
        if (active) setExecCount({ status: "error" });
      }
    }
    loadExecCount();
    const id = window.setInterval(loadExecCount, EXEC_COUNT_REFRESH_MS);
    return () => {
      active = false;
      window.clearInterval(id);
    };
  }, [daemonOk]);

  function startDaemon() {
    if (!onStartDaemon || daemonStartRef.current) return;
    daemonStartRef.current = true;
    setDaemonStarting(true);
    void Promise.resolve(onStartDaemon()).finally(() => {
      daemonStartRef.current = false;
      if (aliveRef.current) setDaemonStarting(false);
    });
  }

  const connectedHosts = connectionStatuses.filter((c) => c.connected).length;
  const daemonOffline = daemonHealth != null && daemonHealth.ok !== true;
  // A failed audit read and a daemon that is already offline are the same
  // card: "—" plus Unavailable. Loading stays "…" and is never that null.
  const execShown: ExecCountState = daemonOffline ? { status: "error" } : execCount;

  return (
    <div className="grid gap-4">
      <div className="grid grid-cols-3 gap-4 max-lg:grid-cols-2 max-md:grid-cols-1">
        <StatCard
          icon={<Server size={13} />}
          label={t("Host health")}
          value={`${connectedHosts}/${hosts.length}`}
          hint={t("connected")}
          tone={hosts.length > 0 && connectedHosts === 0 ? "warning" : "default"}
          onClick={() => onNavigateModule("hosts")}
        />
        <StatCard
          icon={<ShieldAlert size={13} />}
          label={t("Pending approvals")}
          value={pendingApprovalsCount}
          tone={pendingApprovalsCount > 0 ? "warning" : "success"}
          onClick={() => onNavigateModule("approvals")}
        />
        <StatCard
          icon={<AlertTriangle size={13} />}
          label={t("Anomaly alerts")}
          value={anomalyCount}
          hint={
            anomalyStreamStatus === "live"
              ? t("since dashboard opened")
              : anomalyStreamStatus === "connecting"
                ? t("connecting")
                : t("stream offline")
          }
          tone={anomalyCount > 0 ? "warning" : "default"}
          onClick={onOpenAnomalies ?? (() => onNavigateModule("activity"))}
        />
        <StatCard
          icon={<RadioTower size={13} />}
          label={t("24h executions")}
          value={execShown.status === "loading" ? "…" : execShown.status === "error" ? "—" : execShown.count}
          hint={execShown.status === "error" ? t("Unavailable") : undefined}
          onClick={() => onNavigateModule("audit")}
        />
        <StatCard
          icon={secretsLocked ? <Lock size={13} /> : <LockOpen size={13} />}
          label={t("Credential store")}
          value={secretsLocked ? t("Locked") : t("Unlocked")}
          tone={secretsLocked ? "warning" : "success"}
          onClick={
            secretsLocked
              ? onRequestUnlock
              : () => onNavigateModule("keys")
          }
        />
        <StatCard
          icon={daemonOk ? <CheckCircle2 size={13} /> : <XCircle size={13} />}
          label={t("Local daemon")}
          value={
            daemonOk
              ? t("Daemon running")
              : daemonStarting
                ? t("Starting...")
                : t("Daemon offline")
          }
          hint={daemonOk && daemonHealth?.version ? `v${daemonHealth.version}` : undefined}
          tone={daemonOk ? "success" : daemonStarting ? "warning" : "destructive"}
          busy={daemonStarting}
          onClick={
            daemonOk
              ? () => onNavigateModule("activity")
              : onStartDaemon
                ? startDaemon
                : undefined
          }
        />
      </div>

      {/* Connectivity sweep: content-area display for the per-host ping results.
          It lived in the sidebar rail (crushed labels) and then a popover
          (full-height overlay for a 37-host fleet) before landing here. */}
      <PingPanel
        hosts={hosts}
        onOpenTerminal={onOpenHostTerminal}
        onInspectHost={onInspectHost}
      />
    </div>
  );
}

export default memo(Dashboard);
