import { ArrowLeftRight, Loader2, Plus, RefreshCw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { ForwardDirection, ForwardRule, HostProfile } from "../types";
import { Badge } from "./ui/badge";
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
  onChanged?: () => void | Promise<void>;
};

export default function ForwardPanel({ hosts, initialHost = "", onChanged }: Props) {
  const { t } = useI18n();
  const [tunnelHost, setTunnelHost] = useState(initialHost);
  const [rules, setRules] = useState<ForwardRule[]>([]);
  const [direction, setDirection] = useState<ForwardDirection>("local");
  const [bindPort, setBindPort] = useState(8080);
  const [destinationHost, setDestinationHost] = useState("localhost");
  const [targetPort, setTargetPort] = useState(80);
  const [busy, setBusy] = useState(false);
  const [removingId, setRemovingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const canAdd = Boolean(tunnelHost) && isValidPort(bindPort) && isValidPort(targetPort);

  const activeHostNames = useMemo(() => new Set(rules.map((rule) => rule.host)), [rules]);

  const refresh = useCallback(async () => {
    try {
      const list = await api.forwardList();
      setRules(list);
    } catch (err) {
      setError(String(err));
      reportError("forward-panel", "list forwards failed", err);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    if (!tunnelHost && initialHost) {
      setTunnelHost(initialHost);
    }
  }, [initialHost, tunnelHost]);

  async function addForward() {
    if (!canAdd) return;
    setBusy(true);
    setError(null);
    try {
      await api.forwardAdd(tunnelHost, direction, bindPort, destinationHost, targetPort);
      await refresh();
      await onChanged?.();
    } catch (err) {
      setError(String(err));
      reportError("forward-panel", "add forward failed", err, { host: tunnelHost, direction });
    } finally {
      setBusy(false);
    }
  }

  async function removeForward(id: string) {
    setRemovingId(id);
    setError(null);
    try {
      await api.forwardRemove(id);
      await refresh();
      await onChanged?.();
    } catch (err) {
      setError(String(err));
      reportError("forward-panel", "remove forward failed", err);
    } finally {
      setRemovingId(null);
    }
  }

  return (
    <div className="grid grid-cols-[minmax(340px,0.48fr)_minmax(0,1fr)] items-start gap-[18px] max-lg:grid-cols-1">
      <Card className="space-y-3 p-4">
        <div className="flex items-center gap-2 font-semibold">
          <Plus size={16} className="text-muted-foreground" />
          {t("New tunnel")}
        </div>

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
          <Button onClick={addForward} disabled={busy || !canAdd} className="w-full">
            {busy ? <Loader2 size={14} className="animate-spin" /> : <Plus size={14} />}
            {busy ? t("Adding...") : t("Add tunnel")}
          </Button>
        </div>
      </Card>

      <Card className="space-y-3 p-4">
        <div className="flex items-center gap-2 font-semibold">
          <ArrowLeftRight size={16} className="text-muted-foreground" />
          {t("Tunnel List")}
          <Badge variant={rules.length > 0 ? "success" : "secondary"} className="ml-1 font-medium">
            {t("{count} active", { count: rules.length })}
          </Badge>
          <IconButton className="ml-auto" onClick={refresh} title={t("Refresh")}>
            <RefreshCw size={15} />
          </IconButton>
        </div>

        {error && (
          <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2.5 text-sm text-destructive">
            {error}
          </div>
        )}

        <div className="grid gap-2">
          {rules.map((rule) => {
            const removing = removingId === rule.id;
            return (
              <div
                key={rule.id}
                className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 rounded-lg border border-border bg-card px-3 py-2.5"
              >
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <Badge variant={rule.direction === "local" ? "default" : "warning"}>
                      {rule.direction === "local" ? "-L" : "-R"}
                    </Badge>
                    <Badge variant="success">{t("Active")}</Badge>
                    <span className="text-xs text-muted-foreground">{rule.host}</span>
                  </div>
                  <div className="mt-2 grid gap-1.5 text-sm">
                    <TunnelEndpoint
                      label={
                        rule.direction === "local" ? t("Local listener") : t("Remote listener")
                      }
                      value={`${rule.direction === "local" ? "127.0.0.1" : rule.host}:${
                        rule.bind_port
                      }`}
                    />
                    <TunnelEndpoint
                      label={t("Destination")}
                      value={`${rule.target_host}:${rule.target_port}`}
                    />
                  </div>
                </div>
                <IconButton
                  onClick={() => removeForward(rule.id)}
                  title={t("Remove tunnel")}
                  disabled={removing}
                >
                  {removing ? <Loader2 size={14} className="animate-spin" /> : <Trash2 size={14} />}
                </IconButton>
              </div>
            );
          })}
          {rules.length === 0 && (
            <div className="rounded-lg border border-dashed border-border bg-muted/30 px-3 py-6 text-center text-sm text-muted-foreground">
              {t("No active tunnels")}
            </div>
          )}
        </div>

        {activeHostNames.size > 0 && (
          <div className="text-xs text-muted-foreground">
            {t("{count} hosts with tunnels", { count: activeHostNames.size })}
          </div>
        )}
      </Card>
    </div>
  );
}

function TunnelEndpoint({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid grid-cols-[112px_minmax(0,1fr)] items-center gap-2 max-sm:grid-cols-1">
      <span className="text-xs font-medium text-muted-foreground">{label}</span>
      <code className="min-w-0 break-all rounded-md bg-muted px-2 py-1 font-mono text-xs text-foreground">
        {value}
      </code>
    </div>
  );
}

function isValidPort(port: number): boolean {
  return Number.isInteger(port) && port >= 1 && port <= 65535;
}
