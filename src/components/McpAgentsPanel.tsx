import { ArrowUpCircle, BookOpen, Bot, CheckCircle2, Clipboard, Code2, PlugZap, RefreshCw, ShieldAlert, Trash2 } from "lucide-react";
import { memo, useEffect, useMemo, useState } from "react";
import agyLogo from "../assets/logos/agy.png";
import claudeLogo from "../assets/logos/claude.png";
import codebuddyLogo from "../assets/logos/codebuddy.png";
import codexLogo from "../assets/logos/codex.svg";
import cursorLogo from "../assets/logos/cursor.png";
import qoderLogo from "../assets/logos/qoder.png";
import qoderCnLogo from "../assets/logos/qoder-cn.png";
import traeLogo from "../assets/logos/trae.png";
import windsurfLogo from "../assets/logos/windsurf.png";
import workbuddyLogo from "../assets/logos/workbuddy.png";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { CliPathStatus, McpAgentConfigStatus, SkillHarnessStatus } from "../types";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { Spinner } from "./ui/state";
import { useToast } from "./ui/toast";
import { confirmDialog } from "./ui/dialog";

const chipCls = "max-w-full truncate rounded bg-muted px-1.5 py-1 text-xs text-muted-foreground";

const AGENT_LOGOS: Record<string, string> = {
  agy: agyLogo,
  claude_code: claudeLogo,
  claude_desktop: claudeLogo,
  codex: codexLogo,
  cursor: cursorLogo,
  codebuddy: codebuddyLogo,
  windsurf: windsurfLogo,
  workbuddy: workbuddyLogo,
  qoder: qoderLogo,
  qoder_cn: qoderCnLogo,
  trae: traeLogo,
  trae_cn: traeLogo,
};

function AgentLogo({ id }: { id: string }) {
  const src = AGENT_LOGOS[id];
  if (!src) return null;
  return <img src={src} alt="" width={18} height={18} className="size-[18px] shrink-0 rounded-[4px]" />;
}

function McpAgentsPanel() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [agents, setAgents] = useState<McpAgentConfigStatus[]>([]);
  const [loading, setLoading] = useState(false);
  const [configuring, setConfiguring] = useState<string | null>(null);
  const [uninstalling, setUninstalling] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [harnesses, setHarnesses] = useState<SkillHarnessStatus[]>([]);
  const [skillBusy, setSkillBusy] = useState<string | null>(null);
  const [cli, setCli] = useState<CliPathStatus | null>(null);
  const [cliBusy, setCliBusy] = useState(false);
  const [mode, setMode] = useState<"mcp" | "cli">("cli");
  const [scannedAt, setScannedAt] = useState<string | null>(null);

  const detectedCount = useMemo(() => agents.filter((agent) => agent.detected).length, [agents]);
  const configuredCount = useMemo(
    () => agents.filter((agent) => agent.configured).length,
    [agents]
  );

  async function refresh() {
    setLoading(true);
    let scanned = false;
    try {
      setAgents(await api.listMcpAgentConfigs());
      scanned = true;
    } catch (err) {
      showToast("error", String(err));
      reportError("mcp-agents-panel", "list mcp agent configs failed", err);
    }
    try {
      setHarnesses(await api.listSkillHarnesses());
      scanned = true;
    } catch (err) {
      showToast("error", String(err));
      reportError("mcp-agents-panel", "list skill harnesses failed", err);
    }
    try {
      setCli(await api.getCliPathStatus());
    } catch (err) {
      reportError("mcp-agents-panel", "cli path status failed", err);
    } finally {
      if (scanned) setScannedAt(new Date().toLocaleTimeString());
      setLoading(false);
    }
  }

  async function toggleCliPath() {
    setCliBusy(true);
    try {
      const status = cli?.installed ? await api.removeCliFromPath() : await api.installCliToPath();
      setCli(status);
      showToast("success", status.message);
    } catch (err) {
      showToast("error", String(err));
      reportError("mcp-agents-panel", "cli path change failed", err);
    } finally {
      setCliBusy(false);
    }
  }

  function replaceHarness(next: SkillHarnessStatus) {
    setHarnesses((current) => current.map((item) => (item.id === next.id ? next : item)));
  }

  async function installSkill(harness: SkillHarnessStatus) {
    setSkillBusy(harness.id);
    try {
      const status = await api.installSkillHarness(harness.id);
      replaceHarness(status);
      showToast("success", t("Agent skill installed to {path}", { path: status.path }));
    } catch (err) {
      showToast("error", String(err));
      reportError("mcp-agents-panel", "install agent skill failed", err, { harness: harness.id });
    } finally {
      setSkillBusy(null);
    }
  }

  async function uninstallSkill(harness: SkillHarnessStatus) {
    const ok = await confirmDialog({
      title: t("Remove the Agent2SSH skill from {name}?", { name: harness.name }),
      confirmLabel: t("Uninstall"),
      danger: true,
    });
    if (!ok) return;
    setSkillBusy(harness.id);
    try {
      const status = await api.uninstallSkillHarness(harness.id);
      replaceHarness(status);
      showToast("success", t("Agent skill removed"));
    } catch (err) {
      showToast("error", String(err));
      reportError("mcp-agents-panel", "uninstall agent skill failed", err, { harness: harness.id });
    } finally {
      setSkillBusy(null);
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
    const confirmed = await confirmDialog({
      title: t("Remove the Agent2SSH MCP binding from {name}? Restart that agent client afterward.", {
        name: agent.name,
      }),
      confirmLabel: t("Uninstall"),
      danger: true,
    });
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
    // Same guard as the other copy buttons: absent outside a secure context,
    // where `await` on it rejects and the confirmation below never runs.
    const clipboard = navigator.clipboard;
    if (!clipboard?.writeText) {
      return;
    }
    await clipboard.writeText(agent.recommended_command);
    setCopied(agent.id);
    window.setTimeout(() => setCopied(null), 1600);
  }

  return (
    <Card className="space-y-3.5 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="flex items-center gap-2 font-semibold">
          <Bot size={16} className="text-muted-foreground" />
          {t("MCP/CLI")}
        </h3>
        <div className="ml-auto flex flex-wrap items-center gap-1">
          <Button
            size="sm"
            variant={mode === "cli" ? "default" : "secondary"}
            onClick={() => setMode("cli")}
          >
            {t("Skills + CLI")}
          </Button>
          <Button
            size="sm"
            variant={mode === "mcp" ? "default" : "secondary"}
            onClick={() => setMode("mcp")}
          >
            {t("MCP")}
          </Button>
          <Button variant="secondary" size="sm" onClick={() => void refresh()} disabled={loading}>
            {loading ? <Spinner /> : <RefreshCw />}
            {t("Refresh")}
          </Button>
          {scannedAt && (
            <span className="text-xs text-muted-foreground">{t("Scanned {time}", { time: scannedAt })}</span>
          )}
        </div>
      </div>

      {mode === "mcp" && <div className="grid grid-cols-2 gap-2.5">
        <div className="rounded-lg border border-border bg-muted/40 p-3">
          <strong className="block text-2xl leading-none">{detectedCount}</strong>
          <span className="mt-1 block text-xs text-muted-foreground">{t("detected")}</span>
        </div>
        <div className="rounded-lg border border-border bg-muted/40 p-3">
          <strong className="block text-2xl leading-none">{configuredCount}</strong>
          <span className="mt-1 block text-xs text-muted-foreground">{t("configured")}</span>
        </div>
      </div>}

      <p className="text-xs text-muted-foreground">
        {mode === "cli"
          ? t(
              "Each harness has its own skill directory. Install writes the skill there; a harness is detected from its app, its config directory, or its CLI on PATH."
            )
          : t(
              "Registers agent2ssh-mcp in the client. Those tools stay in the harness context for every session."
            )}
      </p>

      {mode === "cli" && cli && (
        <div className="flex items-center justify-between gap-3.5 rounded-lg border border-border bg-card p-3 max-md:flex-col max-md:items-stretch">
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <Code2 size={16} className="text-muted-foreground" />
              <strong>{t("CLI")}</strong>
              <Badge variant={cli.installed ? "success" : "secondary"}>
                {cli.installed ? t("On PATH") : t("Not on PATH")}
              </Badge>
            </div>
            <div className="mt-2 flex flex-wrap gap-1.5">
              <code className={chipCls} title={cli.cliPath}>
                {cli.cliPath}
              </code>
            </div>
            <p className="mt-1.5 text-xs text-muted-foreground">
              {cli.message} {t("Open a new terminal after changing PATH.")}
            </p>
            <code className="mt-2 block truncate text-[11px] text-muted-foreground">
              {t("Example")} agent2ssh local exec -H local:zsh &quot;cargo build&quot; --json
            </code>
          </div>
          <div className="flex items-center gap-2 max-md:justify-end">
            <Button size="sm" onClick={toggleCliPath} disabled={cliBusy || !cli.cliExists}>
              {cliBusy ? t("Working...") : cli.installed ? t("Remove CLI from PATH") : t("Add CLI to PATH")}
            </Button>
          </div>
        </div>
      )}

      {mode === "cli" && (
        <div className="grid gap-2.5">
          {harnesses.map((harness) => {
            const busy = skillBusy !== null;
            return (
              <div
                key={harness.id}
                className="flex items-center justify-between gap-3.5 rounded-lg border border-border bg-card p-3 max-md:flex-col max-md:items-stretch"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <AgentLogo id={harness.id} />
                    <strong>{harness.name}</strong>
                    <Badge variant={harness.detected ? "default" : "secondary"}>
                      {harness.detected ? t("Detected") : t("Not detected")}
                    </Badge>
                    <Badge
                      variant={
                        !harness.installed
                          ? "secondary"
                          : harness.update_available
                            ? "warning"
                            : "success"
                      }
                    >
                      {!harness.installed
                        ? t("Not installed")
                        : harness.update_available
                          ? t("Update available")
                          : t("Installed")}
                    </Badge>
                    {harness.installed && harness.installed_version && (
                      <Badge variant="outline">v{harness.installed_version}</Badge>
                    )}
                  </div>
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    <span className={chipCls} title={harness.path}>
                      {harness.dir}
                    </span>
                    {harness.available_version && (
                      <code className={chipCls}>
                        {t("bundled: v{version}", { version: harness.available_version })}
                      </code>
                    )}
                  </div>
                </div>
                <div className="flex items-center gap-2 max-md:justify-end">
                  <Button size="sm" onClick={() => installSkill(harness)} disabled={busy}>
                    {harness.update_available ? <ArrowUpCircle size={14} /> : <BookOpen size={14} />}
                    {skillBusy === harness.id
                      ? t("Working...")
                      : harness.update_available
                        ? t("Update")
                        : harness.installed
                          ? t("Reinstall")
                          : t("Install")}
                  </Button>
                  <Button
                    variant="destructive"
                    size="sm"
                    onClick={() => uninstallSkill(harness)}
                    disabled={busy || !harness.installed}
                    title={t("Remove the installed agent skill")}
                  >
                    <Trash2 size={14} />
                    {t("Uninstall")}
                  </Button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {mode === "mcp" && <div className="grid gap-2.5">
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
                  <AgentLogo id={agent.id} />
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
      </div>}
    </Card>
  );
}

export default memo(McpAgentsPanel);
