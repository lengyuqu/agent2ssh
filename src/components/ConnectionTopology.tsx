import {
  forceCenter,
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  type Simulation,
  type SimulationNodeDatum,
} from "d3-force";
import { RefreshCw, Waypoints } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { ConnectionStatus, DaemonHealth, ForwardRule, HostProfile, ProxyProfile } from "../types";
import { Card } from "./ui/card";
import { IconButton } from "./ui/icon-button";
import { EmptyState } from "./ui/state";

type NodeKind = "daemon" | "host" | "proxy" | "endpoint";
type Tone = "success" | "warning" | "destructive" | "muted" | "primary";

type GraphNode = SimulationNodeDatum & {
  id: string;
  kind: NodeKind;
  label: string;
  tone: Tone;
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
};

function hostTone(name: string, statuses: ConnectionStatus[]): Tone {
  const status = statuses.find((s) => s.host === name);
  if (!status || !status.connected) return "muted";
  if (status.reconnecting) return "warning";
  if (status.healthy === false) return "destructive";
  return "success";
}

/** V4-1: force-directed connection topology — daemon → host → proxy/jump-host/tunnel. */
export default function ConnectionTopology({
  hosts,
  proxies,
  connectionStatuses,
  daemonHealth,
  onSelectHost,
  onNavigateModule,
}: Props) {
  const { t } = useI18n();
  const [forwards, setForwards] = useState<ForwardRule[]>([]);
  const containerRef = useRef<HTMLDivElement>(null);
  const nodeElRefs = useRef<Map<string, SVGGElement>>(new Map());
  const linkElRefs = useRef<Map<number, SVGLineElement>>(new Map());
  const simulationRef = useRef<Simulation<GraphNode, undefined> | null>(null);
  const [size, setSize] = useState({ width: 800, height: 480 });
  const [hoveredHost, setHoveredHost] = useState<string | null>(null);

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
    if (!el) return;
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) {
        setSize({ width: entry.contentRect.width, height: Math.max(360, entry.contentRect.height) });
      }
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const { nodes, links } = useMemo<{ nodes: GraphNode[]; links: GraphLink[] }>(() => {
    const nodeList: GraphNode[] = [
      {
        id: "daemon",
        kind: "daemon",
        label: t("Local daemon"),
        tone: daemonHealth?.ok ? "success" : "destructive",
      },
    ];
    const linkList: GraphLink[] = [];
    const hostNames = new Set(hosts.map((h) => h.name));

    for (const host of hosts) {
      nodeList.push({
        id: `host:${host.name}`,
        kind: "host",
        label: host.name,
        tone: hostTone(host.name, connectionStatuses),
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
        tone: "primary",
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
          tone: "muted",
        });
      }
      linkList.push({ source: `host:${rule.host}`, target: endpointId, dashed: true });
    }

    // Drop links whose endpoints don't exist (e.g. a forward's host was removed).
    const nodeIds = new Set(nodeList.map((n) => n.id));
    return { nodes: nodeList, links: linkList.filter((l) => nodeIds.has(l.source) && nodeIds.has(l.target)) };
  }, [hosts, proxies, forwards, connectionStatuses, daemonHealth, t]);

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

    const simulation = forceSimulation<GraphNode>(liveNodes)
      .force(
        "link",
        forceLink<GraphNode, GraphLink>(links)
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
      links.forEach((link, index) => {
        const el = linkElRefs.current.get(index);
        if (!el) return;
        const source = liveNodes.find((n) => n.id === link.source || n.id === (link.source as unknown as GraphNode).id);
        const target = liveNodes.find((n) => n.id === link.target || n.id === (link.target as unknown as GraphNode).id);
        if (source && target) {
          el.setAttribute("x1", String(source.x ?? 0));
          el.setAttribute("y1", String(source.y ?? 0));
          el.setAttribute("x2", String(target.x ?? 0));
          el.setAttribute("y2", String(target.y ?? 0));
        }
      });
    });

    simulationRef.current = simulation;
    return () => {
      simulation.stop();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nodes, links, size.width, size.height]);

  return (
    <Card className="space-y-3 p-4">
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

      <div ref={containerRef} className="min-h-[420px] w-full overflow-hidden rounded-lg border border-border bg-muted/20">
        {hosts.length === 0 ? (
          <EmptyState icon={Waypoints} title={t("No hosts configured")} />
        ) : (
          <svg width="100%" height={size.height} viewBox={`0 0 ${size.width} ${size.height}`}>
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
                const isHost = node.kind === "host";
                const hostName = isHost ? node.label : null;
                return (
                  <g
                    key={node.id}
                    ref={(el) => {
                      if (el) nodeElRefs.current.set(node.id, el);
                      else nodeElRefs.current.delete(node.id);
                    }}
                    className={isHost ? "cursor-pointer" : undefined}
                    onClick={
                      isHost && hostName
                        ? () => {
                            onSelectHost(hostName);
                            onNavigateModule("hosts");
                          }
                        : undefined
                    }
                    onPointerEnter={isHost && hostName ? () => setHoveredHost(hostName) : undefined}
                    onPointerLeave={isHost ? () => setHoveredHost(null) : undefined}
                  >
                    <circle
                      r={RADIUS[node.kind]}
                      fill={TONE_VAR[node.tone]}
                      fillOpacity={node.kind === "daemon" || node.kind === "host" ? 0.85 : 0.35}
                      stroke={TONE_VAR[node.tone]}
                      strokeWidth={hoveredHost === node.label ? 3 : 1.5}
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
    </Card>
  );
}
