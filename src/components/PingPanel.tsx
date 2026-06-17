import { Radio, Wifi, WifiOff } from "lucide-react";
import { useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import type { HostProfile, PingResult } from "../types";

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
    <section className="panel ping-panel">
      <div className="panel-title">
        <Radio size={16} />
        {t("Connectivity")}
        <button
          className="icon-button"
          onClick={pingAll}
          disabled={busy || hosts.length === 0}
          title={t("Ping all hosts")}
        >
          {busy ? "..." : <Radio size={15} />}
        </button>
      </div>
      {error && <div className="error">{error}</div>}

      <div className="ping-list">
        {results.map((r) => (
          <div
            key={r.host}
            className={`ping-row ${r.reachable ? "ping-ok" : "ping-fail"}`}
          >
            {r.reachable ? <Wifi size={14} /> : <WifiOff size={14} />}
            <strong>{r.host}</strong>
            {r.latency_ms != null && (
              <span className="ping-latency">{r.latency_ms}ms</span>
            )}
            {r.error && <span className="ping-error">{r.error}</span>}
          </div>
        ))}
        {results.length === 0 && (
          <div className="empty">{t("Click the icon to ping all hosts")}</div>
        )}
      </div>
    </section>
  );
}
