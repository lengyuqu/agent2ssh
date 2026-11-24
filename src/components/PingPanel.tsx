import { Radio, Wifi, WifiOff } from "lucide-react";
import { memo, useMemo, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { HostProfile, PingResult } from "../types";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { Spinner } from "./ui/state";
import { useToast } from "./ui/toast";
import { cn } from "../lib/utils";

type Props = {
  hosts: HostProfile[];
  onOpenTerminal?: (host: string) => void;
  onInspectHost?: (host: string) => void;
};

function latencyRank(result: PingResult): number {
  return typeof result.latency_ms === "number" ? result.latency_ms : Number.NEGATIVE_INFINITY;
}

/** Unreachable hosts first, then higher latency first. Equal rows keep probe order. */
function comparePingResults(a: PingResult, b: PingResult): number {
  if (a.reachable !== b.reachable) return a.reachable ? 1 : -1;
  return latencyRank(b) - latencyRank(a);
}

/**
 * Connectivity sweep, rendered as a card on the Dashboard. The ping button
 * probes every host; the verdicts list below scrolls under a fixed cap so a
 * large fleet (40+ hosts) cannot stretch the page. This display deliberately
 * lives in the content area — the earlier sidebar/popover variants either
 * crushed the narrow rail or floated a full-height overlay over it.
 */
function PingPanel({ hosts, onOpenTerminal, onInspectHost }: Props) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [results, setResults] = useState<PingResult[]>([]);
  const [busy, setBusy] = useState(false);
  const [problemsOnly, setProblemsOnly] = useState(false);

  async function pingAll() {
    if (hosts.length === 0 || busy) return;
    setBusy(true);
    try {
      const names = hosts.map((h) => h.name);
      setResults(await api.pingHosts(names));
    } catch (err) {
      showToast("error", String(err));
      reportError("ping-panel", "ping hosts failed", err);
    } finally {
      setBusy(false);
    }
  }

  const ordered = useMemo(() => {
    return results
      .map((result, index) => ({ result, index }))
      .sort((a, b) => {
        const byVerdict = comparePingResults(a.result, b.result);
        return byVerdict !== 0 ? byVerdict : a.index - b.index;
      })
      .map(({ result }) => result);
  }, [results]);
  const visible = problemsOnly ? ordered.filter((result) => !result.reachable) : ordered;
  const reachable = results.filter((r) => r.reachable).length;

  return (
    <Card className="space-y-3 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="flex items-center gap-2 font-semibold">
          <Radio size={16} className="text-muted-foreground" />
          {t("Connectivity")}
        </h3>
        {results.length > 0 && !busy && (
          <Badge
            variant={reachable === results.length ? "success" : "destructive"}
            className="font-mono"
          >
            {reachable}/{results.length}
          </Badge>
        )}
        {results.length > 0 && (
          <div className="flex items-center gap-1">
            <Button
              variant={problemsOnly ? "outline" : "secondary"}
              size="sm"
              aria-pressed={!problemsOnly}
              onClick={() => setProblemsOnly(false)}
            >
              {t("All")}
            </Button>
            <Button
              variant={problemsOnly ? "secondary" : "outline"}
              size="sm"
              aria-pressed={problemsOnly}
              onClick={() => setProblemsOnly(true)}
            >
              {t("Problems only")}
            </Button>
          </div>
        )}
        <Button
          variant="secondary"
          size="sm"
          className="ml-auto"
          onClick={() => void pingAll()}
          disabled={busy || hosts.length === 0}
          title={t("Ping all hosts")}
        >
          {busy ? <Spinner size={13} /> : <Radio size={14} />}
          {t("Ping all hosts")}
        </Button>
      </div>

      {results.length > 0 ? (
        visible.length > 0 ? (
          <div className="grid max-h-72 gap-1 overflow-y-auto pr-1" role="list">
            {visible.map((r) => (
              <div
                key={r.host}
                role="listitem"
                className={cn(
                  "flex items-center gap-2 rounded-md px-2.5 py-1.5 text-xs",
                  r.reachable
                    ? "bg-success/15 text-success"
                    : "bg-destructive/15 text-destructive"
                )}
              >
                {r.reachable ? <Wifi size={13} /> : <WifiOff size={13} />}
                <strong className="min-w-0 flex-1 truncate font-semibold">{r.host}</strong>
                {r.latency_ms != null && (
                  <span className="shrink-0 text-muted-foreground">{r.latency_ms}ms</span>
                )}
                {r.error && (
                  <span className="min-w-0 max-w-[30%] truncate text-destructive/80">{r.error}</span>
                )}
                {onOpenTerminal && (
                  <Button
                    variant={r.reachable ? "secondary" : "outline"}
                    size="sm"
                    className="h-7 shrink-0 px-2"
                    aria-label={`${t("Open terminal")} ${r.host}`}
                    onClick={() => onOpenTerminal(r.host)}
                  >
                    {t("Open terminal")}
                  </Button>
                )}
                {onInspectHost && (
                  <Button
                    variant="outline"
                    size="sm"
                    className="h-7 shrink-0 px-2"
                    aria-label={`${t("View host")} ${r.host}`}
                    onClick={() => onInspectHost(r.host)}
                  >
                    {t("View host")}
                  </Button>
                )}
              </div>
            ))}
          </div>
        ) : (
          <p className="text-xs text-muted-foreground">{t("No unreachable hosts")}</p>
        )
      ) : (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          {busy && <Spinner size={12} />}
          {t("Click the icon to ping all hosts")}
        </div>
      )}
    </Card>
  );
}

export default memo(PingPanel);
