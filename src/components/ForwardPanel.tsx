import { ArrowLeftRight, Plus, RefreshCw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import type { ForwardDirection, ForwardRule } from "../types";

type Props = {
  selectedHost: string;
};

export default function ForwardPanel({ selectedHost }: Props) {
  const { t } = useI18n();
  const [rules, setRules] = useState<ForwardRule[]>([]);
  const [direction, setDirection] = useState<ForwardDirection>("local");
  const [bindPort, setBindPort] = useState(8080);
  const [targetHost, setTargetHost] = useState("localhost");
  const [targetPort, setTargetPort] = useState(80);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const list = await api.forwardList();
      setRules(list);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  async function addForward() {
    if (!selectedHost) return;
    setBusy(true);
    setError(null);
    try {
      await api.forwardAdd(
        selectedHost,
        direction,
        bindPort,
        targetHost,
        targetPort
      );
      await refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function removeForward(id: string) {
    try {
      await api.forwardRemove(id);
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <section className="panel forward-panel">
      <div className="panel-title">
        <ArrowLeftRight size={16} />
        {t("Port Forwarding")}
        <button className="icon-button" onClick={refresh} title={t("Refresh")}>
          <RefreshCw size={15} />
        </button>
      </div>
      {error && <div className="error">{error}</div>}

      <div className="forward-form">
        <div className="forward-form-row">
          <label>
            {t("Direction")}
            <select
              value={direction}
              onChange={(e) =>
                setDirection(e.target.value as ForwardDirection)
              }
            >
              <option value="local">{t("Local (-L)")}</option>
              <option value="remote">{t("Remote (-R)")}</option>
            </select>
          </label>
          <label>
            {t("Bind port")}
            <input
              type="number"
              min={1}
              max={65535}
              value={bindPort}
              onChange={(e) => setBindPort(Number(e.target.value))}
            />
          </label>
        </div>
        <div className="forward-form-row">
          <label>
            {t("Target host")}
            <input
              value={targetHost}
              onChange={(e) => setTargetHost(e.target.value)}
              placeholder="localhost"
            />
          </label>
          <label>
            {t("Target port")}
            <input
              type="number"
              min={1}
              max={65535}
              value={targetPort}
              onChange={(e) => setTargetPort(Number(e.target.value))}
            />
          </label>
        </div>
        <button
          className="primary"
          onClick={addForward}
          disabled={busy || !selectedHost}
        >
          <Plus size={14} />
          {busy ? t("Adding...") : t("Add tunnel")}
        </button>
      </div>

      <div className="forward-list">
        {rules.map((rule) => (
          <div key={rule.id} className="forward-row">
            <span className="forward-direction">
              {rule.direction === "local" ? "-L" : "-R"}
            </span>
            <code>
              :{rule.bind_port} → {rule.target_host}:{rule.target_port}
            </code>
            <span className="forward-host">{rule.host}</span>
            <button
              className="icon-button"
              onClick={() => removeForward(rule.id)}
              title={t("Remove tunnel")}
            >
              <Trash2 size={14} />
            </button>
          </div>
        ))}
        {rules.length === 0 && (
          <div className="empty">{t("No active tunnels")}</div>
        )}
      </div>
    </section>
  );
}
