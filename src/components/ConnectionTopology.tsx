import {
  forceCenter,
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  type Simulation,
  type SimulationNodeDatum,
} from "d3-force";
import { ArrowLeftRight, Play, RefreshCw, Server, Terminal, Waypoints, X } from "lucide-react";
import { memo, useEffect, useMemo, useRef, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { ConnectionStatus, DaemonHealth, ForwardRule, HostProfile, ProxyProfile } from "../types";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { IconButton } from "./ui/icon-button";
import { EmptyState } from "./ui/state";

type NodeKind = "daemon" | "host" | "proxy" | "endpoint";
type Tone = "success" | "warning" | "destructive" | "muted" | "primary";

type GraphNode = SimulationNodeDatum & {
  id: string;
  kind: NodeKind;
  label: string;
  detail?: string;
};

type GraphLink = { source: string; target: string; dashed?: boolean };

const TONE_VAR: Record<Tone, string> = {
  success: "var(--success)",
  warning: "var(--warning)",
  destructive: "var(--destructive)",
  muted: "var(--muted-foreground)",
  primary: "var(--primary)",
};

const RADIUS: Record<NodeKind, number> = {
  daemon: 20,
  host: 14,
  proxy: 11,
  endpoint: 8,
};

type Props = {
  hosts: HostProfile[];
  proxies: ProxyProfile[];
  connectionStatuses: ConnectionStatus[];
  daemonHealth: DaemonHealth | null;
  onSelectHost: (name: string) => void;
  onNavigateModule: (id: string) => void;
  onOpenTerminal?: (host: string) => void;
  onOpenExecute?: (host: string) => void;
};

function hostTone(status: ConnectionStatus | undefined): Tone {
  if (!status || !status.connected) return "muted";
  if (status.reconnecting) return "warning";
  if (status.healthy === false) return "destructive";
  return "success";
}

/** V4-1: force-directed connection topology — daemon → host → proxy/jump-host/tunnel. */
function ConnectionTopology({
  hosts,
  proxies,
  connectionStatuses,
  daemonHealth,
  onSelectHost,
  onNavigateModule,
  onOpenTerminal,
  onOpenExecute,
}: Props) {
  const { t } = useI18n();
  const [forwards, setForwards] = useState<ForwardRule[]>([]);
  const containerRef = useRef<HTMLDivElement>(null);
  const nodeElRefs = useRef<Map<string, SVGGElement>>(new Map());
  const linkElRefs = useRef<Map<number, SVGLineElement>>(new Map());
  const simulationRef = useRef<Simulation<GraphNode, undefined> | null>(null);
  const [size, setSize] = useState({ width: 800, height: 480 });
  const [hoveredNodeId, setHoveredNodeId] = useState<string | null>(null);
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);

  async function refreshForwards() {
    try {
      setForwards(await api.forwardList());
    } catch (err) {
      reportError("connection-topology", "list forwards failed", err);
    }
  }

  useEffect(() => {
    refreshForwards();
  }, []);

  useEffect(() => {
    const el = containerRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) {
        setSize({ width: entry.contentRect.width, height: Math.max(360, entry.contentRect.height) });
      }
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // Layout structure only — tones live in `toneById` below so a status-poll
  // color change repaints the SVG without recreating the force simulation.
  const { nodes, links } = useMemo<{ nodes: GraphNode[]; links: GraphLink[] }>(() => {
    const nodeList: GraphNode[] = [
      {
        id: "daemon",
        kind: "daemon",
        label: t("Local daemon"),
      },
    ];
    const linkList: GraphLink[] = [];
    const hostNames = new Set(hosts.map((h) => h.name));

    for (const host of hosts) {
      nodeList.push({
        id: `host:${host.name}`,
        kind: "host",
        label: host.name,
        detail: host.host,
      });
      linkList.push({ source: "daemon", target: `host:${host.name}` });

      if (host.jump_host && hostNames.has(host.jump_host)) {
        linkList.push({
          source: `host:${host.name}`,
          target: `host:${host.jump_host}`,
          dashed: true,
        });
      }
      if (host.proxy_id) {
        linkList.push({ source: `host:${host.name}`, target: `proxy:${host.proxy_id}` });
      }
    }

    for (const proxy of proxies) {
      nodeList.push({
        id: `proxy:${proxy.id}`,
        kind: "proxy",
        label: proxy.name,
        detail: `${proxy.protocol} ${proxy.host}:${proxy.port}`,
      });
    }

    const endpointIds = new Set<string>();
    for (const rule of forwards) {
      const endpointId = `endpoint:${rule.target_host}:${rule.target_port}`;
      if (!endpointIds.has(endpointId)) {
        endpointIds.add(endpointId);
        nodeList.push({
          id: endpointId,
          kind: "endpoint",
          label: `${rule.target_host}:${rule.target_port}`,
        });
      }
      linkList.push({ source: `host:${rule.host}`, target: endpointId, dashed: true });
    }

    // Drop links whose endpoints don't exist (e.g. a forward's host was removed).
    const nodeIds = new Set(nodeList.map((n) => n.id));
    return { nodes: nodeList, links: linkList.filter((l) => nodeIds.has(l.source) && nodeIds.has(l.target)) };
  }, [hosts, proxies, forwards, t]);

  const toneById = useMemo<Map<string, Tone>>(() => {
    const statusByHost = new Map(connectionStatuses.map((s) => [s.host, s]));
    const map = new Map<string, Tone>();
    map.set("daemon", daemonHealth?.ok ? "success" : "destructive");
    for (const host of hosts) map.set(`host:${host.name}`, hostTone(statusByHost.get(host.name)));
    for (const proxy of proxies) map.set(`proxy:${proxy.id}`, "primary");
    return map;
  }, [hosts, proxies, connectionStatuses, daemonHealth]);

  useEffect(() => {
    if (nodes.length === 0) return;
    // Reuse positions from the previous simulation's nodes (keyed by id) so
    // re-running on data changes doesn't reset the whole layout each time.
    const previous = new Map(
      (simulationRef.current?.nodes() ?? []).map((n) => [n.id, n])
    );
    const liveNodes: GraphNode[] = nodes.map((n) => {
      const prior = previous.get(n.id);
      return prior ? { ...n, x: prior.x, y: prior.y, vx: prior.vx, vy: prior.vy } : n;
    });
    // Hand d3 its own copies: forceLink mutates source/target from id strings
    // into node references, and those must point at *this* run's nodes — not
    // leak into (or reuse stale references from) the memoized `links` array.
    const liveLinks = links.map((link) => ({ ...link }));

    const simulation = forceSimulation<GraphNode>(liveNodes)
      .force(
        "link",
        forceLink<GraphNode, GraphLink>(liveLinks)
          .id((n) => n.id)
          .distance(80)
      )
      .force("charge", forceManyBody().strength(-260))
      .force("center", forceCenter(size.width / 2, size.height / 2))
      .force("collide", forceCollide((n) => RADIUS[(n as GraphNode).kind] + 18));

    simulation.on("tick", () => {
      for (const node of liveNodes) {
        const el = nodeElRefs.current.get(node.id);
        if (el) el.setAttribute("transform", `translate(${node.x ?? 0}, ${node.y ?? 0})`);
      }
      liveLinks.forEach((link, index) => {
        const el = linkElRefs.current.get(index);
        if (!el) return;
        // By the first tick, forceLink has replaced the id strings with
        // references to this run's nodes.
        const source = link.source as unknown as GraphNode;
        const target = link.target as unknown as GraphNode;
        el.setAttribute("x1", String(source.x ?? 0));
        el.setAttribute("y1", String(source.y ?? 0));
        el.setAttribute("x2", String(target.x ?? 0));
        el.setAttribute("y2", String(target.y ?? 0));
      });
    });

    simulationRef.current = simulation;
    return () => {
      simulation.stop();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nodes, links, size.width, size.height]);

  const selectedNode = useMemo(
    () => (selectedNodeId ? nodes.find((node) => node.id === selectedNodeId) ?? null : null),
    [nodes, selectedNodeId],
  );
  const statusByHost = useMemo(
    () => new Map(connectionStatuses.map((status) => [status.host, status])),
    [connectionStatuses],
  );
  const connectedCount = useMemo(
    () => connectionStatuses.filter((status) => status.connected).length,
    [connectionStatuses],
  );

  function renderInspector() {
    if (!selectedNode) return null;

    if (selectedNode.kind === "host") {
      const host = hosts.find((item) => item.name === selectedNode.label);
      if (!host) return null;
      const status = statusByHost.get(host.name);
      const tone = hostTone(status);
      const proxy = host.proxy_id ? proxies.find((item) => item.id === host.proxy_id) : null;
      const hostForwards = forwards.filter((rule) => rule.host === host.name);
      return (
        <aside
          aria-label={t("Node details")}
          className="flex w-72 shrink-0 flex-col gap-3 overflow-y-auto border-l border-border bg-card p-3.5 max-lg:w-full max-lg:border-l-0 max-lg:border-t"
        >
          <div className="flex items-start gap-2">
            <div className="min-w-0 flex-1">
              <div className="truncate font-semibold text-foreground">{host.name}</div>
              <div className="truncate font-mono text-xs text-muted-foreground">
                {host.user ? `${host.user}@` : ""}
                {host.host}:{host.port ?? 22}
              </div>
            </div>
            <IconButton size="sm" title={t("Close")} onClick={() => setSelectedNodeId(null)}>
              <X size={14} />
            </IconButton>
          </div>

          <div className="flex flex-wrap items-center gap-1.5">
            <Badge
              variant={
                tone === "success"
                  ? "success"
                  : tone === "warning"
                    ? "warning"
                    : tone === "destructive"
                      ? "destructive"
                      : "secondary"
              }
            >
              {t(
                tone === "success"
                  ? "Connected"
                  : tone === "warning"
                    ? "Reconnecting"
                    : tone === "destructive"
                      ? "Stale"
                      : "Disconnected",
              )}
            </Badge>
            {host.env && <Badge variant="outline">{host.env}</Badge>}
            {host.role && <Badge variant="outline">{host.role}</Badge>}
          </div>

          {(host.jump_host || proxy) && (
            <div className="grid gap-1.5 text-xs">
              {host.jump_host && (
                <div className="flex items-center justify-between gap-2">
                  <span className="text-muted-foreground">{t("Jump host")}</span>
                  <button
                    type="button"
                    className="truncate font-mono text-primary hover:underline"
                    onClick={() => setSelectedNodeId(`host:${host.jump_host}`)}
                  >
                    {host.jump_host}
                  </button>
                </div>
              )}
              {proxy && (
                <div className="flex items-center justify-between gap-2">
                  <span className="text-muted-foreground">{t("Proxy")}</span>
                  <button
                    type="button"
                    className="truncate font-mono text-primary hover:underline"
                    onClick={() => setSelectedNodeId(`proxy:${proxy.id}`)}
                  >
                    {proxy.name} ({proxy.protocol})
                  </button>
                </div>
              )}
            </div>
          )}

          {hostForwards.length > 0 && (
            <div className="grid gap-1 text-xs">
              <div className="font-semibold text-muted-foreground">
                {t("Tunnels")} ({hostForwards.length})
              </div>
              {hostForwards.map((rule) => (
                <button
                  key={rule.id}
                  type="button"
                  className="truncate rounded border border-border bg-muted/30 px-2 py-1 text-left font-mono text-[11px] hover:bg-muted"
                  onClick={() => setSelectedNodeId(`endpoint:${rule.target_host}:${rule.target_port}`)}
                >
                  {rule.direction === "local" ? "-L" : "-R"} :{rule.bind_port} → {rule.target_host}:{rule.target_port}
                </button>
              ))}
            </div>
          )}

          <div className="mt-auto grid gap-1.5 border-t border-border pt-3">
            {onOpenTerminal && (
              <Button size="sm" block onClick={() => onOpenTerminal(host.name)}>
                <Terminal size={14} />
                {t("Open terminal")}
              </Button>
            )}
            <div className="grid grid-cols-2 gap-1.5">
              {onOpenExecute && (
                <Button variant="secondary" size="sm" onClick={() => onOpenExecute(host.name)}>
                  <Play size={13} />
                  {t("Execute")}
                </Button>
              )}
              <Button
                variant="outline"
                size="sm"
                onClick={() => {
                  onSelectHost(host.name);
                  onNavigateModule("hosts");
                }}
              >
                <Server size={13} />
                {t("View host")}
              </Button>
            </div>
          </div>
        </aside>
      );
    }

    if (selectedNode.kind === "proxy") {
      const proxyId = selectedNode.id.slice("proxy:".length);
      const proxy = proxies.find((item) => item.id === proxyId);
      if (!proxy) return null;
      const assignedHosts = hosts.filter((item) => item.proxy_id === proxy.id);
      return (
        <aside
          aria-label={t("Node details")}
          className="flex w-72 shrink-0 flex-col gap-3 overflow-y-auto border-l border-border bg-card p-3.5 max-lg:w-full max-lg:border-l-0 max-lg:border-t"
        >
          <div className="flex items-start gap-2">
            <div className="min-w-0 flex-1">
              <div className="truncate font-semibold text-foreground">{proxy.name}</div>
              <div className="truncate font-mono text-xs text-muted-foreground">
                {proxy.protocol.toUpperCase()} · {proxy.host}:{proxy.port}
              </div>
            </div>
            <IconButton size="sm" title={t("Close")} onClick={() => setSelectedNodeId(null)}>
              <X size={14} />
            </IconButton>
          </div>
          {assignedHosts.length > 0 && (
            <div className="grid gap-1 text-xs">
              <div className="font-semibold text-muted-foreground">
                {t("{count} hosts", { count: assignedHosts.length })}
              </div>
              <div className="flex flex-wrap gap-1">
                {assignedHosts.map((host) => (
                  <button
                    key={host.name}
                    type="button"
                    className="rounded-full border border-border bg-muted px-2 py-0.5 text-xs hover:text-primary"
                    onClick={() => setSelectedNodeId(`host:${host.name}`)}
                  >
                    {host.name}
                  </button>
                ))}
              </div>
            </div>
          )}
          <div className="mt-auto border-t border-border pt-3">
            <Button variant="secondary" size="sm" block onClick={() => onNavigateModule("proxies")}>
              {t("Proxies")}
            </Button>
          </div>
        </aside>
      );
    }

    if (selectedNode.kind === "endpoint") {
      const matching = forwards.filter(
        (rule) => `endpoint:${rule.target_host}:${rule.target_port}` === selectedNode.id,
      );
      return (
        <aside
          aria-label={t("Node details")}
          className="flex w-72 shrink-0 flex-col gap-3 overflow-y-auto border-l border-border bg-card p-3.5 max-lg:w-full max-lg:border-l-0 max-lg:border-t"
        >
          <div className="flex items-start gap-2">
            <div className="min-w-0 flex-1">
              <div className="truncate font-mono text-sm font-semibold text-foreground">
                {selectedNode.label}
              </div>
              <div className="text-xs text-muted-foreground">{t("Destination")}</div>
            </div>
            <IconButton size="sm" title={t("Close")} onClick={() => setSelectedNodeId(null)}>
              <X size={14} />
            </IconButton>
          </div>
          <div className="grid gap-1.5 text-xs">
            {matching.map((rule) => (
              <div key={rule.id} className="rounded border border-border bg-muted/30 p-2 font-mono">
                <div className="flex items-center justify-between gap-2">
                  <Badge variant={rule.direction === "local" ? "default" : "warning"}>
                    {rule.direction === "local" ? "-L" : "-R"}
                  </Badge>
                  <button
                    type="button"
                    className="truncate text-primary hover:underline"
                    onClick={() => setSelectedNodeId(`host:${rule.host}`)}
                  >
                    {rule.host}
                  </button>
                </div>
                <div className="mt-1 text-muted-foreground">
                  :{rule.bind_port} → {rule.target_host}:{rule.target_port}
                </div>
              </div>
            ))}
          </div>
          <div className="mt-auto border-t border-border pt-3">
            <Button variant="secondary" size="sm" block onClick={() => onNavigateModule("tunnels")}>
              <ArrowLeftRight size={14} />
              {t("Tunnels")}
            </Button>
          </div>
        </aside>
      );
    }

    return (
      <aside
        aria-label={t("Node details")}
        className="flex w-72 shrink-0 flex-col gap-3 overflow-y-auto border-l border-border bg-card p-3.5 max-lg:w-full max-lg:border-l-0 max-lg:border-t"
      >
        <div className="flex items-start gap-2">
          <div className="min-w-0 flex-1">
            <div className="font-semibold text-foreground">{t("Local daemon")}</div>
            {daemonHealth?.version && (
              <div className="font-mono text-xs text-muted-foreground">v{daemonHealth.version}</div>
            )}
          </div>
          <IconButton size="sm" title={t("Close")} onClick={() => setSelectedNodeId(null)}>
            <X size={14} />
          </IconButton>
        </div>
        <div className="flex flex-wrap gap-1.5">
          <Badge variant={daemonHealth?.ok ? "success" : "destructive"}>
            {daemonHealth?.ok ? t("Daemon running") : t("Daemon offline")}
          </Badge>
          <Badge variant="secondary">
            {t("{connected}/{total} hosts connected", {
              connected: connectedCount,
              total: hosts.length,
            })}
          </Badge>
        </div>
        <div className="mt-auto border-t border-border pt-3">
          <Button variant="secondary" size="sm" block onClick={() => onNavigateModule("activity")}>
            {t("Activity")}
          </Button>
        </div>
      </aside>
    );
  }

  return (
    <Card className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden p-4">
      <div className="flex items-center gap-2 font-semibold">
        <Waypoints size={16} className="text-muted-foreground" />
        {t("Connection topology")}
        <IconButton className="ml-auto" onClick={refreshForwards} title={t("Refresh")}>
          <RefreshCw size={15} />
        </IconButton>
      </div>

      <div className="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
        {(["success", "warning", "destructive", "muted"] as Tone[]).map((tone) => (
          <span key={tone} className="inline-flex items-center gap-1.5">
            <span className="size-2.5 rounded-full" style={{ background: TONE_VAR[tone] }} />
            {t(
              tone === "success"
                ? "Connected"
                : tone === "warning"
                  ? "Reconnecting"
                  : tone === "destructive"
                    ? "Stale"
                    : "Disconnected"
            )}
          </span>
        ))}
        <span className="inline-flex items-center gap-1.5">
          <span className="size-2.5 rounded-full" style={{ background: TONE_VAR.primary }} />
          {t("Proxy")}
        </span>
      </div>

      <div className="flex min-h-0 flex-1 overflow-hidden rounded-lg border border-border bg-muted/20 max-lg:flex-col">
        <div ref={containerRef} className="min-h-0 min-w-0 flex-1 overflow-hidden">
          {hosts.length === 0 ? (
            <EmptyState icon={Waypoints} title={t("No hosts configured")} />
          ) : (
            <svg
              width="100%"
              height={size.height}
              viewBox={`0 0 ${size.width} ${size.height}`}
              onClick={() => setSelectedNodeId(null)}
            >
              <g>
                {links.map((link, index) => (
                  <line
                    key={`${link.source}-${link.target}-${index}`}
                    ref={(el) => {
                      if (el) linkElRefs.current.set(index, el);
                      else linkElRefs.current.delete(index);
                    }}
                    stroke="var(--border)"
                    strokeWidth={1.5}
                    strokeDasharray={link.dashed ? "4 3" : undefined}
                    opacity={0.7}
                  />
                ))}
              </g>
              <g>
                {nodes.map((node) => {
                  const tone = toneById.get(node.id) ?? "muted";
                  const emphasized = hoveredNodeId === node.id || selectedNodeId === node.id;
                  return (
                    <g
                      key={node.id}
                      ref={(el) => {
                        if (el) nodeElRefs.current.set(node.id, el);
                        else nodeElRefs.current.delete(node.id);
                      }}
                      className="cursor-pointer"
                      role="button"
                      aria-label={node.label}
                      onClick={(event) => {
                        event.stopPropagation();
                        setSelectedNodeId(node.id);
                      }}
                      onPointerEnter={() => setHoveredNodeId(node.id)}
                      onPointerLeave={() => setHoveredNodeId(null)}
                    >
                      <circle
                        r={RADIUS[node.kind]}
                        fill={TONE_VAR[tone]}
                        fillOpacity={node.kind === "daemon" || node.kind === "host" ? 0.85 : 0.35}
                        stroke={selectedNodeId === node.id ? "var(--foreground)" : TONE_VAR[tone]}
                        strokeWidth={emphasized ? 3 : 1.5}
                      />
                      <text
                        y={RADIUS[node.kind] + 13}
                        textAnchor="middle"
                        fontSize={11}
                        fill="var(--foreground)"
                        className="pointer-events-none select-none"
                      >
                        {node.label}
                      </text>
                    </g>
                  );
                })}
              </g>
            </svg>
          )}
        </div>
        {renderInspector()}
      </div>
    </Card>
  );
}

export default memo(ConnectionTopology);
