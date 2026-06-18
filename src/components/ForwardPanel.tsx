import { ArrowLeftRight, Plus, RefreshCw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import type { ForwardDirection, ForwardRule, HostProfile } from "../types";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import HostSelector from "./HostSelector";
import { IconButton } from "./ui/icon-button";
import { Input } from "./ui/input";
import { Select } from "./ui/select";

const labelCls = "grid gap-1.5 text-sm font-medium text-foreground/90";

type Props = {
  hosts: HostProfile[];
  initialHost?: string;
};

export default function ForwardPanel({ hosts, initialHost = "" }: Props) {
  const { t } = useI18n();
  const [tunnelHost, setTunnelHost] = useState(initialHost);
  const [rules, setRules] = useState<ForwardRule[]>([]);
  const [direction, setDirection] = useState<ForwardDirection>("local");
  const [bindPort, setBindPort] = useState(8080);
  const [destinationHost, setDestinationHost] = useState("localhost");
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
    if (!tunnelHost) return;
    setBusy(true);
    setError(null);
    try {
      await api.forwardAdd(tunnelHost, direction, bindPort, destinationHost, targetPort);
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
    <Card className="space-y-3 p-4">
      <div className="flex items-center gap-2 font-semibold">
        <ArrowLeftRight size={16} className="text-muted-foreground" />
        {t("Port Forwarding")}
        <IconButton className="ml-auto" onClick={refresh} title={t("Refresh")}>
          <RefreshCw size={15} />
        </IconButton>
      </div>
      {error && (
        <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2.5 text-sm text-destructive">
          {error}
        </div>
      )}
      <HostSelector hosts={hosts} value={tunnelHost} onChange={setTunnelHost} disabled={busy} />

      <div className="space-y-2.5">
        <div className="grid grid-cols-2 gap-2.5 max-sm:grid-cols-1">
          <label className={labelCls}>
            {t("Direction")}
            <Select
              value={direction}
              onChange={(e) => setDirection(e.target.value as ForwardDirection)}
            >
              <option value="local">{t("Local (-L)")}</option>
              <option value="remote">{t("Remote (-R)")}</option>
            </Select>
          </label>
          <label className={labelCls}>
            {t("Bind port")}
            <Input
              type="number"
              min={1}
              max={65535}
              value={bindPort}
              onChange={(e) => setBindPort(Number(e.target.value))}
            />
          </label>
        </div>
        <div className="grid grid-cols-2 gap-2.5 max-sm:grid-cols-1">
          <label className={labelCls}>
            {t("Target host")}
            <Input
              value={destinationHost}
              onChange={(e) => setDestinationHost(e.target.value)}
              placeholder="localhost"
            />
          </label>
          <label className={labelCls}>
            {t("Target port")}
            <Input
              type="number"
              min={1}
              max={65535}
              value={targetPort}
              onChange={(e) => setTargetPort(Number(e.target.value))}
            />
          </label>
        </div>
        <Button onClick={addForward} disabled={busy || !tunnelHost} className="w-full">
          <Plus size={14} />
          {busy ? t("Adding...") : t("Add tunnel")}
        </Button>
      </div>

      <div className="grid gap-1.5">
        {rules.map((rule) => (
          <div
            key={rule.id}
            className="flex items-center gap-3 rounded-md border border-border bg-muted/40 px-2.5 py-2"
          >
            <span className="rounded bg-primary/15 px-1.5 py-0.5 font-mono text-[11px] font-bold text-primary">
              {rule.direction === "local" ? "-L" : "-R"}
            </span>
            <code className="text-sm text-foreground">
              :{rule.bind_port} → {rule.target_host}:{rule.target_port}
            </code>
            <span className="ml-auto text-sm text-muted-foreground">{rule.host}</span>
            <IconButton onClick={() => removeForward(rule.id)} title={t("Remove tunnel")}>
              <Trash2 size={14} />
            </IconButton>
          </div>
        ))}
        {rules.length === 0 && (
          <div className="px-1 py-2 text-sm text-muted-foreground">{t("No active tunnels")}</div>
        )}
      </div>
    </Card>
  );
}
