import { Bot, BrainCircuit, CheckCircle2, Clipboard, Code2, Globe, Laptop, PlugZap, RefreshCw, Send, ShieldAlert, Trash2, Users, Wrench } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { McpAgentConfigStatus } from "../types";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { useToast } from "./ui/toast";

const chipCls = "max-w-full truncate rounded bg-muted px-1.5 py-1 text-xs text-muted-foreground";

function agentIconFor(agent: McpAgentConfigStatus) {
  switch (agent.id) {
    case "cursor":
      return Code2;
    case "codebuddy":
      return BrainCircuit;
    case "windsurf":
      return Send;
    case "workbuddy":
      return Users;
    case "qoder_work":
      return Laptop;
    case "trae":
    case "trae_solo":
      return Globe;
    case "claude_desktop":
      return Wrench;
    case "codex":
    default:
      return Bot;
  }
}

export default function McpAgentsPanel() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [agents, setAgents] = useState<McpAgentConfigStatus[]>([]);
  const [loading, setLoading] = useState(false);
  const [configuring, setConfiguring] = useState<string | null>(null);
  const [uninstalling, setUninstalling] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  const detectedCount = useMemo(() => agents.filter((agent) => agent.detected).length, [agents]);
  const configuredCount = useMemo(
    () => agents.filter((agent) => agent.configured).length,
    [agents]
  );

  async function refresh() {
    setLoading(true);
    try {
      setAgents(await api.listMcpAgentConfigs());
    } catch (err) {
      showToast("error", String(err));
      reportError("mcp-agents-panel", "list mcp agent configs failed", err);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function configure(agent: McpAgentConfigStatus) {
    setConfiguring(agent.id);
    try {
      const result = await api.configureMcpAgent(agent.id);
      showToast("success", result.message);
      await refresh();
    } catch (err) {
      showToast("error", String(err));
      reportError("mcp-agents-panel", "configure mcp agent failed", err, { agent: agent.id });
    } finally {
      setConfiguring(null);
    }
  }

  async function uninstall(agent: McpAgentConfigStatus) {
    const confirmed = window.confirm(
      t("Remove the Agent2SSH MCP binding from {name}? Restart that agent client afterward.", {
        name: agent.name,
      })
    );
    if (!confirmed) return;
    setUninstalling(agent.id);
    try {
      const result = await api.uninstallMcpAgent(agent.id);
      showToast("success", result.message);
      await refresh();
    } catch (err) {
      showToast("error", String(err));
      reportError("mcp-agents-panel", "uninstall mcp agent failed", err, { agent: agent.id });
    } finally {
      setUninstalling(null);
    }
  }

  async function copyCommand(agent: McpAgentConfigStatus) {
    await navigator.clipboard.writeText(agent.recommended_command);
    setCopied(agent.id);
    window.setTimeout(() => setCopied(null), 1600);
  }

  return (
    <Card className="space-y-3.5 p-4">
      <div className="flex items-center gap-2">
        <h3 className="flex items-center gap-2 font-semibold">
          <Bot size={16} className="text-muted-foreground" />
          {t("MCP Agents")}
        </h3>
        <Button variant="secondary" size="sm" className="ml-auto" onClick={refresh} disabled={loading}>
          <RefreshCw size={14} className={loading ? "animate-spin" : ""} />
          {t("Refresh")}
        </Button>
      </div>

      <div className="grid grid-cols-2 gap-2.5">
        <div className="rounded-lg border border-border bg-muted/40 p-3">
          <strong className="block text-2xl leading-none">{detectedCount}</strong>
          <span className="mt-1 block text-xs text-muted-foreground">{t("detected")}</span>
        </div>
        <div className="rounded-lg border border-border bg-muted/40 p-3">
          <strong className="block text-2xl leading-none">{configuredCount}</strong>
          <span className="mt-1 block text-xs text-muted-foreground">{t("configured")}</span>
        </div>
      </div>

      <div className="grid gap-2.5">
        {agents.map((agent) => {
          const invalid = agent.status.startsWith("invalid_config");
          const needsRebind = agent.status === "needs_rebind";
          const busy = configuring !== null || uninstalling !== null;
          return (
            <div
              key={agent.id}
              className="flex items-center justify-between gap-3.5 rounded-lg border border-border bg-card p-3 max-md:flex-col max-md:items-stretch"
            >
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  {(() => {
                    const AgentIcon = agentIconFor(agent);
                    return <AgentIcon size={16} className="text-muted-foreground" />;
                  })()}
                  <strong>{agent.name}</strong>
                  <Badge
                    variant={
                      agent.configured ? "success" : needsRebind ? "destructive" : agent.detected ? "default" : "secondary"
                    }
                  >
                    {agent.configured
                      ? t("Configured")
                      : needsRebind
                        ? t("Rebind required")
                        : agent.detected
                        ? t("Detected")
                        : t("Not detected")}
                  </Badge>
                  {invalid && (
                    <Badge variant="destructive">
                      <ShieldAlert size={12} />
                      {t("Invalid config")}
                    </Badge>
                  )}
                </div>
                <div className="mt-2 flex flex-wrap gap-1.5">
                  <span className={chipCls} title={agent.config_path}>
                    {agent.config_path}
                  </span>
                  {agent.command && (
                    <code className={chipCls} title={agent.command}>
                      {agent.command}
                    </code>
                  )}
                  <code
                    className={chipCls}
                    title={
                      agent.configured_source
                        ? `Configured source: ${agent.configured_source}`
                        : `Recommended source: ${agent.source}`
                    }
                  >
                    source: {agent.configured_source ?? agent.source}
                  </code>
                </div>
              </div>
              <div className="flex items-center gap-2 max-md:justify-end">
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => copyCommand(agent)}
                  disabled={busy}
                  title={t("Copy MCP command")}
                >
                  <Clipboard size={14} />
                  {copied === agent.id ? t("Copied") : t("Copy command")}
                </Button>
                <Button
                  size="sm"
                  onClick={() => configure(agent)}
                  disabled={busy || invalid}
                  title={agent.configured ? t("Update MCP config") : t("Configure MCP")}
                >
                  {agent.configured ? <CheckCircle2 size={14} /> : <PlugZap size={14} />}
                  {configuring === agent.id
                    ? t("Configuring...")
                    : agent.configured
                      ? t("Update")
                      : needsRebind
                        ? t("Rebind")
                      : t("Configure")}
                </Button>
                <Button
                  variant="destructive"
                  size="sm"
                  onClick={() => uninstall(agent)}
                  disabled={busy || (!agent.configured && !agent.command) || invalid}
                  title={t("Uninstall MCP binding")}
                >
                  <Trash2 size={14} />
                  {uninstalling === agent.id ? t("Uninstalling...") : t("Uninstall")}
                </Button>
              </div>
            </div>
          );
        })}
      </div>
    </Card>
  );
}
