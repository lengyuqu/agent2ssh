import { Radio, Wifi, WifiOff } from "lucide-react";
import { memo, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { HostProfile, PingResult } from "../types";
import { Spinner } from "./ui/state";
import { useToast } from "./ui/toast";
import { cn } from "../lib/utils";

type Props = {
  hosts: HostProfile[];
};

/**
 * Sidebar connectivity entry, styled like the "Command palette" row: one line,
 * pinned to the bottom of the sidebar. Clicking it pings every host and shows
 * the per-host verdicts in a popover anchored above the row — the old inline
 * result list grew with the host count and pushed the rest of the sidebar out
 * of the viewport (the 150px rail also wrapped its two-line label).
 */
function PingPanel({ hosts }: Props) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [results, setResults] = useState<PingResult[]>([]);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);

  async function pingAll() {
    if (hosts.length === 0 || busy) return;
    setBusy(true);
    setOpen(true);
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

  const reachable = results.filter((r) => r.reachable).length;

  return (
    <div className="relative">
      <button
        type="button"
        className="flex w-full items-center justify-between rounded-lg px-2.5 py-1.5 text-[13px] text-sidebar-foreground/70 transition-colors hover:bg-white/5 hover:text-sidebar-foreground disabled:opacity-50"
        onClick={() => (open ? setOpen(false) : void pingAll())}
        disabled={busy || hosts.length === 0}
        title={t("Ping all hosts")}
      >
        <span className="flex items-center gap-2">
          <Radio size={14} className={cn("shrink-0 opacity-70", busy && "animate-pulse")} />
          {t("Connectivity")}
        </span>
        {results.length > 0 && !busy && (
          <span
            className={cn(
              "rounded-full px-1.5 font-mono text-[10px] font-bold leading-4",
              reachable === results.length
                ? "bg-success/15 text-success"
                : "bg-destructive/15 text-destructive"
            )}
          >
            {reachable}/{results.length}
          </span>
        )}
      </button>

      {open && (
        <>
          {/* Light-dismiss: click anywhere outside the popover to close it. */}
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
          <div className="absolute bottom-full left-0 z-50 mb-2 w-64 rounded-xl border border-edge bg-popover p-2 text-popover-foreground shadow-lg">
            <div className="grid gap-1">
              {results.map((r) => (
                <div
                  key={r.host}
                  className={cn(
                    "flex items-center gap-2 rounded-md px-2 py-1.5 text-xs",
                    r.reachable
                      ? "bg-success/15 text-success"
                      : "bg-destructive/15 text-destructive"
                  )}
                >
                  {r.reachable ? <Wifi size={13} /> : <WifiOff size={13} />}
                  <strong className="min-w-0 truncate font-semibold">{r.host}</strong>
                  {r.latency_ms != null && (
                    <span className="ml-auto shrink-0 text-muted-foreground">
                      {r.latency_ms}ms
                    </span>
                  )}
                  {r.error && (
                    <span className="ml-auto min-w-0 truncate text-destructive/80">
                      {r.error}
                    </span>
                  )}
                </div>
              ))}
              {results.length === 0 && (
                <div className="flex items-center gap-2 px-2 py-1.5 text-xs text-muted-foreground">
                  <Spinner size={12} />
                  {t("Ping all hosts")}…
                </div>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  );
}

export default memo(PingPanel);
