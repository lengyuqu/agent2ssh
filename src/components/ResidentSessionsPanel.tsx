import { RefreshCw, Timer, XCircle } from "lucide-react";
import { useEffect, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { ResidentSessionSummary } from "../types";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { IconButton } from "./ui/icon-button";
import { EmptyState, LoadingState, Spinner } from "./ui/state";
import { useToast } from "./ui/toast";

/**
 * List the daemon's resident sessions — the long-lived shells behind
 * `/resident-sessions`. Read-only plus close: opening and running commands in
 * a session is an execution concern that belongs to the exec surfaces, not to
 * an inventory panel. The data lives in the daemon, so when the daemon is not
 * running the panel says that instead of rendering an empty list.
 */
export default function ResidentSessionsPanel() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [sessions, setSessions] = useState<ResidentSessionSummary[]>([]);
  const [busy, setBusy] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [closing, setClosing] = useState<string | null>(null);

  async function refresh() {
    setBusy(true);
    try {
      setSessions(await api.residentListDaemon());
    } catch (err) {
      showToast("error", String(err));
      reportError("resident-sessions-panel", "resident session list failed", err);
    } finally {
      setBusy(false);
      setLoaded(true);
    }
  }

  useEffect(() => {
    refresh();
    // Mount only, matching the other panels: the refresh button owns re-reads.
  }, []);

  async function close(session: ResidentSessionSummary) {
    setClosing(session.id);
    try {
      await api.residentCloseDaemon(session.id);
      await refresh();
    } catch (err) {
      showToast("error", String(err));
      reportError("resident-sessions-panel", "resident session close failed", err);
    } finally {
      setClosing(null);
    }
  }

  return (
    <Card className="space-y-3.5 p-4">
      <div className="flex items-center gap-2">
        <h3 className="flex items-center gap-2 font-semibold">
          <Timer size={16} className="text-muted-foreground" />
          {t("Resident Sessions")}
        </h3>
        {sessions.length > 0 && (
          <span className="text-xs text-muted-foreground">
            {t("{count} sessions", { count: sessions.length })}
          </span>
        )}
        <IconButton
          className="ml-auto"
          onClick={() => refresh()}
          disabled={busy}
          aria-label={t("Refresh resident sessions")}
          title={t("Refresh resident sessions")}
        >
          {busy ? <Spinner size={14} /> : <RefreshCw size={14} />}
        </IconButton>
      </div>

      {!loaded && busy && <LoadingState label={t("Reading resident sessions...")} />}

      {loaded && sessions.length === 0 && (
        <EmptyState
          icon={Timer}
          title={t("No resident sessions")}
          description={t(
            "Resident sessions are long-lived local shells owned by the daemon. Open one with 'agent2ssh resident open' or the daemon API; they also die with the daemon or after the idle timeout."
          )}
        />
      )}

      <div className="grid gap-2">
        {sessions.map((session) => (
          <div
            key={session.id}
            className="flex flex-wrap items-center gap-2 rounded-md border border-border bg-muted/20 px-2.5 py-2"
          >
            <Badge variant={session.dead ? "outline" : "success"}>
              {session.dead ? t("dead") : t("alive")}
            </Badge>
            <div className="min-w-0 flex-1">
              <div className="truncate text-sm font-medium text-foreground" title={session.host}>
                {session.host}
              </div>
              <div className="truncate text-xs text-muted-foreground" title={session.id}>
                {session.label} · {t("{secs}s idle", { secs: session.idle_secs })}
              </div>
            </div>
            <code className="hidden max-w-[16rem] truncate rounded bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground lg:block">
              {session.id}
            </code>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => close(session)}
              disabled={closing === session.id}
              title={t("Close session")}
            >
              <XCircle size={14} />
              {closing === session.id ? t("Closing...") : t("Close session")}
            </Button>
          </div>
        ))}
      </div>
    </Card>
  );
}
