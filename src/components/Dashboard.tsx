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
import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { useAgentEvents, useEventsStatus } from "../eventsBus";
import { useI18n } from "../i18n";
import { cn } from "../lib/utils";
import type { ConnectionStatus, DaemonHealth, HostProfile } from "../types";
import { Card, CardContent, CardHeader, CardTitle } from "./ui/card";

const EXEC_COUNT_REFRESH_MS = 60_000;

type DashboardProps = {
  hosts: HostProfile[];
  connectionStatuses: ConnectionStatus[];
  pendingApprovalsCount: number;
  secretsLocked: boolean;
  daemonHealth: DaemonHealth | null;
  onNavigateModule: (id: string) => void;
};

type StatCardProps = {
  icon: React.ReactNode;
  label: string;
  value: React.ReactNode;
  hint?: string;
  tone?: "default" | "success" | "warning" | "destructive";
  onClick?: () => void;
};

function StatCard({ icon, label, value, hint, tone = "default", onClick }: StatCardProps) {
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
export default function Dashboard({
  hosts,
  connectionStatuses,
  pendingApprovalsCount,
  secretsLocked,
  daemonHealth,
  onNavigateModule,
}: DashboardProps) {
  const { t } = useI18n();
  const [execCount24h, setExecCount24h] = useState<number | null>(null);
  const [anomalyCount, setAnomalyCount] = useState(0);
  // V2-1: shared event bus (single SSE connection) instead of a dashboard-local
  // subscription — LiveActivityPanel and NotificationCenter use the same one.
  const anomalyStreamStatus = useEventsStatus();
  const mountedAtRef = useRef(Date.now());

  useAgentEvents((event) => {
    if (event.event_type === "anomaly_detected") {
      setAnomalyCount((count) => count + 1);
    }
  });

  useEffect(() => {
    let active = true;
    async function loadExecCount() {
      try {
        const since = new Date(Date.now() - 24 * 60 * 60 * 1000).toISOString();
        const entries = await api.listAudit({ since, limit: 5000 });
        if (active) setExecCount24h(entries.length);
      } catch {
        if (active) setExecCount24h(null);
      }
    }
    loadExecCount();
    const id = window.setInterval(loadExecCount, EXEC_COUNT_REFRESH_MS);
    return () => {
      active = false;
      window.clearInterval(id);
    };
  }, []);

  const connectedHosts = connectionStatuses.filter((c) => c.connected).length;
  const daemonOk = daemonHealth?.ok === true;

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
        />
        <StatCard
          icon={<RadioTower size={13} />}
          label={t("24h executions")}
          value={execCount24h ?? "…"}
          onClick={() => onNavigateModule("audit")}
        />
        <StatCard
          icon={secretsLocked ? <Lock size={13} /> : <LockOpen size={13} />}
          label={t("Credential store")}
          value={secretsLocked ? t("Locked") : t("Unlocked")}
          tone={secretsLocked ? "warning" : "success"}
        />
        <StatCard
          icon={daemonOk ? <CheckCircle2 size={13} /> : <XCircle size={13} />}
          label={t("Local daemon")}
          value={daemonOk ? t("Daemon running") : t("Daemon offline")}
          hint={daemonHealth?.version ? `v${daemonHealth.version}` : undefined}
          tone={daemonOk ? "success" : "destructive"}
        />
      </div>
    </div>
  );
}
