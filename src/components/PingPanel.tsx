import { Radio, Wifi, WifiOff } from "lucide-react";
import { useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import type { HostProfile, PingResult } from "../types";
import { cn } from "../lib/utils";

type Props = {
  hosts: HostProfile[];
};

export default function PingPanel({ hosts }: Props) {
  const { t } = useI18n();
  const [results, setResults] = useState<PingResult[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function pingAll() {
    if (hosts.length === 0) return;
    setBusy(true);
    setError(null);
    try {
      const names = hosts.map((h) => h.name);
      const res = await api.pingHosts(names);
      setResults(res);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="rounded-lg border border-white/10 bg-white/5 p-3">
      <div className="flex items-center gap-2 text-sm font-semibold text-sidebar-foreground">
        <Radio size={16} className="opacity-70" />
        {t("Connectivity")}
        <button
          className="ml-auto inline-flex size-7 items-center justify-center rounded-md border border-white/15 text-sidebar-foreground/80 transition-colors hover:bg-white/10 disabled:opacity-50"
          onClick={pingAll}
          disabled={busy || hosts.length === 0}
          title={t("Ping all hosts")}
        >
          {busy ? "..." : <Radio size={14} />}
        </button>
      </div>
      {error && (
        <div className="mt-2 rounded-md bg-red-500/20 px-2.5 py-1.5 text-xs text-red-200">
          {error}
        </div>
      )}

      <div className="mt-2 grid gap-1.5">
        {results.map((r) => (
          <div
            key={r.host}
            className={cn(
              "flex items-center gap-2 rounded px-2 py-1.5 text-xs",
              r.reachable ? "bg-emerald-500/15 text-emerald-300" : "bg-red-500/15 text-red-300"
            )}
          >
            {r.reachable ? <Wifi size={14} /> : <WifiOff size={14} />}
            <strong className="font-semibold">{r.host}</strong>
            {r.latency_ms != null && (
              <span className="ml-auto text-sidebar-foreground/50">{r.latency_ms}ms</span>
            )}
            {r.error && <span className="ml-auto truncate text-red-300/80">{r.error}</span>}
          </div>
        ))}
        {results.length === 0 && (
          <div className="text-xs text-sidebar-foreground/45">
            {t("Click the icon to ping all hosts")}
          </div>
        )}
      </div>
    </section>
  );
}
