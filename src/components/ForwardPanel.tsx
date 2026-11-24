import { ArrowLeftRight, Copy, Edit3, Plus, RefreshCw, Trash2 } from "lucide-react";
import { memo, useCallback, useEffect, useMemo, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import { formatBytes } from "../lib/format";
import { labelCls } from "../lib/ui-classes";
import type { ForwardDirection, ForwardRule, ForwardRuleStats, HostProfile } from "../types";
import HostSelector from "./HostSelector";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { IconButton } from "./ui/icon-button";
import { Input } from "./ui/input";
import { Select } from "./ui/select";
import { SlideOverDrawer } from "./ui/slide-over-drawer";
import { EmptyState, Spinner } from "./ui/state";
import { useToast } from "./ui/toast";

type Props = {
  hosts: HostProfile[];
  initialHost?: string;
  onChanged?: () => void | Promise<void>;
  /** Jump to the host that owns a tunnel. */
  onSelectHost?: (hostName: string) => void;
};

function keepIfEqual<T>(prev: T, next: T): T {
  return JSON.stringify(prev) === JSON.stringify(next) ? prev : next;
}

function ForwardPanel({ hosts, initialHost = "", onChanged, onSelectHost }: Props) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [tunnelHost, setTunnelHost] = useState(initialHost);
  const [rules, setRules] = useState<ForwardRule[]>([]);
  const [stats, setStats] = useState<Record<string, ForwardRuleStats>>({});
  const [direction, setDirection] = useState<ForwardDirection>("local");
  const [bindPort, setBindPort] = useState(8080);
  const [destinationHost, setDestinationHost] = useState("localhost");
  const [targetPort, setTargetPort] = useState(80);
  const [via, setVia] = useState("");
  const [busy, setBusy] = useState(false);
  const [removingId, setRemovingId] = useState<string | null>(null);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [drawerMode, setDrawerMode] = useState<"create" | "copy" | "edit">("create");
  const [sourceRule, setSourceRule] = useState<ForwardRule | null>(null);
  const [hostFilter, setHostFilter] = useState("");

  const canAdd = Boolean(tunnelHost) && isValidPort(bindPort) && isValidPort(targetPort);
  const hasRules = rules.length > 0;

  const tunnelHostNames = useMemo(() => {
    const names: string[] = [];
    for (const rule of rules) {
      if (!names.includes(rule.host)) names.push(rule.host);
    }
    return names;
  }, [rules]);

  const visibleRules = useMemo(
    () => (hostFilter ? rules.filter((rule) => rule.host === hostFilter) : rules),
    [rules, hostFilter],
  );

  const activeHostNames = useMemo(
    () =>
      new Set(
        visibleRules
          .filter((rule) => stats[rule.id]?.state === "active" || stats[rule.id]?.state === "starting")
          .map((rule) => rule.host),
      ),
    [visibleRules, stats],
  );
  const activeRuleCount = useMemo(
    () =>
      visibleRules.filter(
        (rule) => stats[rule.id]?.state === "active" || stats[rule.id]?.state === "starting",
      ).length,
    [visibleRules, stats],
  );

  const refresh = useCallback(async () => {
    try {
      const [list, currentStats] = await Promise.all([api.forwardList(), api.forwardStats()]);
      setRules(list);
      setStats((prev) => keepIfEqual(prev, currentStats));
    } catch (err) {
      showToast("error", String(err));
      reportError("forward-panel", "list forwards failed", err);
    }
  }, [showToast]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Traffic moves while the page sits open. Poll only when a rule exists and
  // the document is visible, and keep the previous stats object when nothing
  // changed so a quiet tunnel does not re-render every tick. A missed tick
  // stays quiet; Refresh still reports a real failure.
  useEffect(() => {
    if (!hasRules) return;
    let timer: number | null = null;
    let cancelled = false;

    const poll = () => {
      if (document.visibilityState !== "visible") return;
      void api
        .forwardStats()
        .then((next) => {
          if (cancelled) return;
          setStats((prev) => keepIfEqual(prev, next));
        })
        .catch(() => {
          // Silent on purpose: a background tick must not toast over the list.
        });
    };

    const stop = () => {
      if (timer !== null) {
        window.clearInterval(timer);
        timer = null;
      }
    };

    const start = () => {
      stop();
      if (document.visibilityState !== "visible") return;
      timer = window.setInterval(poll, 3000);
    };

    const onVisibility = () => {
      if (document.visibilityState === "visible") start();
      else stop();
    };

    start();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      cancelled = true;
      stop();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [hasRules]);

  useEffect(() => {
    if (!tunnelHost && initialHost) {
      setTunnelHost(initialHost);
    }
  }, [initialHost, tunnelHost]);

  useEffect(() => {
    if (hostFilter && !tunnelHostNames.includes(hostFilter)) {
      setHostFilter("");
    }
  }, [hostFilter, tunnelHostNames]);

  function openCreate() {
    setDrawerMode("create");
    setSourceRule(null);
    setDirection("local");
    setBindPort(8080);
    setDestinationHost("localhost");
    setTargetPort(80);
    setVia("");
    setDrawerOpen(true);
  }

  function populateFromRule(rule: ForwardRule) {
    setTunnelHost(rule.host);
    setDirection(rule.direction);
    setBindPort(rule.bind_port);
    setDestinationHost(rule.target_host);
    setTargetPort(rule.target_port);
    setVia(rule.via ?? "");
  }

  function openCopy(rule: ForwardRule) {
    setDrawerMode("copy");
    setSourceRule(null);
    populateFromRule(rule);
    setDrawerOpen(true);
  }

  function openEdit(rule: ForwardRule) {
    setDrawerMode("edit");
    setSourceRule(rule);
    populateFromRule(rule);
    setDrawerOpen(true);
  }

  function closeDrawer() {
    setDrawerOpen(false);
  }

  async function addForward() {
    if (!canAdd) return;
    setBusy(true);
    const replacing = drawerMode === "edit" ? sourceRule : null;
    try {
      // Editing a tunnel without changing its listener port must stop the old
      // listener first, otherwise binding the same port fails with EADDRINUSE.
      const sameListener =
        replacing !== null &&
        direction === replacing.direction &&
        bindPort === replacing.bind_port &&
        (direction === "local" || tunnelHost === replacing.host);

      if (replacing && sameListener) {
        await api.forwardRemove(replacing.id);
        try {
          await api.forwardAdd(
            tunnelHost,
            direction,
            bindPort,
            destinationHost,
            targetPort,
            via,
          );
        } catch (addErr) {
          // Best-effort restore so a failed rebind does not leave the user with no tunnel.
          await api
            .forwardAdd(
              replacing.host,
              replacing.direction,
              replacing.bind_port,
              replacing.target_host,
              replacing.target_port,
              replacing.via ?? "",
            )
            .catch(() => {});
          throw addErr;
        }
      } else {
        const created = await api.forwardAdd(
          tunnelHost,
          direction,
          bindPort,
          destinationHost,
          targetPort,
          via,
        );
        if (replacing && created.id !== replacing.id) {
          await api.forwardRemove(replacing.id);
        }
      }
      await refresh();
      await onChanged?.();
      setDrawerOpen(false);
      setSourceRule(null);
      setDrawerMode("create");
    } catch (err) {
      showToast("error", String(err));
      reportError("forward-panel", "add forward failed", err, { host: tunnelHost, direction });
    } finally {
      setBusy(false);
    }
  }

  async function removeForward(id: string) {
    setRemovingId(id);
    try {
      await api.forwardRemove(id);
      if (sourceRule?.id === id) {
        setSourceRule(null);
        setDrawerMode("create");
        setDrawerOpen(false);
      }
      await refresh();
      await onChanged?.();
    } catch (err) {
      showToast("error", String(err));
      reportError("forward-panel", "remove forward failed", err);
    } finally {
      setRemovingId(null);
    }
  }

  return (
    <div>
      <Card className="space-y-3 p-4">
        <div className="flex flex-wrap items-center gap-2">
          <div className="flex items-center gap-2 font-semibold">
            <ArrowLeftRight size={16} className="text-muted-foreground" />
            {t("Tunnel List")}
            <Badge variant={activeRuleCount > 0 ? "success" : "secondary"} className="font-medium">
              {t("{count} active", { count: activeRuleCount })}
            </Badge>
          </div>
          <div className="ml-auto flex flex-wrap items-center gap-2">
            {tunnelHostNames.length > 0 && (
              <Select
                aria-label={t("Filter by host")}
                className="h-8 w-auto min-w-36"
                value={hostFilter}
                onChange={(event) => setHostFilter(event.target.value)}
              >
                <option value="">{t("All hosts")}</option>
                {tunnelHostNames.map((name) => (
                  <option key={name} value={name}>
                    {name}
                  </option>
                ))}
              </Select>
            )}
            <IconButton onClick={refresh} title={t("Refresh")}>
              <RefreshCw size={15} />
            </IconButton>
            <Button onClick={openCreate}>
              <Plus size={14} />
              {t("New tunnel")}
            </Button>
          </div>
        </div>

        <div className="grid gap-2">
          {visibleRules.map((rule) => {
            const removing = removingId === rule.id;
            const ruleStats = stats[rule.id];
            const state = ruleStats?.state ?? "error";
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
                    <Badge
                      variant={
                        state === "active"
                          ? "success"
                          : state === "error" || state === "stopping_error"
                            ? "destructive"
                            : "secondary"
                      }
                    >
                      {state === "active"
                        ? t("Active")
                        : state === "error"
                          ? t("Failed")
                          : state === "starting"
                            ? t("Starting")
                            : state === "stopping_error"
                              ? t("Stop Error")
                              : t("Stopped")}
                    </Badge>
                    {onSelectHost ? (
                      <button
                        type="button"
                        className="text-xs text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
                        onClick={() => onSelectHost(rule.host)}
                      >
                        {rule.host}
                      </button>
                    ) : (
                      <span className="text-xs text-muted-foreground">{rule.host}</span>
                    )}
                  </div>
                  <div className="mt-2 grid gap-1.5 text-sm">
                    <TunnelEndpoint
                      label={rule.direction === "local" ? t("Local listener") : t("Remote listener")}
                      value={`${rule.direction === "local" ? "127.0.0.1" : rule.host}:${rule.bind_port}`}
                    />
                    <TunnelEndpoint
                      label={t("Destination")}
                      value={`${rule.target_host}:${rule.target_port}`}
                    />
                    {ruleStats && (
                      <TunnelEndpoint
                        label={t("Traffic")}
                        value={t("{count} connections · {tx} sent · {rx} received", {
                          count: ruleStats.connections,
                          tx: formatBytes(ruleStats.bytes_tx),
                          rx: formatBytes(ruleStats.bytes_rx),
                        })}
                      />
                    )}
                  </div>
                </div>
                <div className="flex items-center gap-1">
                  <IconButton title={t("Edit tunnel")} onClick={() => openEdit(rule)}>
                    <Edit3 size={14} />
                  </IconButton>
                  <IconButton title={t("Copy into new tunnel")} onClick={() => openCopy(rule)}>
                    <Copy size={14} />
                  </IconButton>
                  <IconButton
                    onClick={() => removeForward(rule.id)}
                    title={t("Remove tunnel")}
                    disabled={removing}
                  >
                    {removing ? <Spinner size={14} /> : <Trash2 size={14} />}
                  </IconButton>
                </div>
              </div>
            );
          })}
          {visibleRules.length === 0 && (
            <EmptyState
              icon={ArrowLeftRight}
              title={rules.length === 0 ? t("No active tunnels") : t("No tunnels for this host")}
            />
          )}
        </div>

        {activeHostNames.size > 0 && (
          <div className="text-xs text-muted-foreground">
            {t("{count} hosts with tunnels", { count: activeHostNames.size })}
          </div>
        )}
      </Card>

      <SlideOverDrawer
        open={drawerOpen}
        onClose={closeDrawer}
        title={
          drawerMode === "edit"
            ? t("Edit tunnel")
            : drawerMode === "copy"
              ? t("Copy tunnel")
              : t("New tunnel")
        }
      >
        <div className="space-y-2.5">
          <HostSelector hosts={hosts} value={tunnelHost} onChange={setTunnelHost} disabled={busy} />

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
          <HostSelector
            hosts={hosts.filter((host) => host.name !== tunnelHost)}
            value={via}
            onChange={setVia}
            label={t("Jump host (optional)")}
            disabled={busy}
            allowEmpty
            emptyOption={t("Direct connection")}
            optionLabel={(host) => host.name}
            shell="plain"
          />
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
          <Button onClick={addForward} disabled={busy || !canAdd} block>
            {busy ? <Spinner size={14} /> : <Plus size={14} />}
            {busy ? t("Adding...") : drawerMode === "edit" ? t("Replace tunnel") : t("Add tunnel")}
          </Button>
        </div>
      </SlideOverDrawer>
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

export default memo(ForwardPanel);
