import { ChevronDown, ChevronRight, Cpu, FileCode2, Play } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";

const EXPANDED_KEY = "agent2ssh.localEnvExpanded";

type LocalEnvInfo = {
  id: string;
  builtin: boolean;
  base: string | null;
  distro: string | null;
  cwd: string | null;
  env_count: number;
  description: string | null;
  platform?: string;
};

type Props = {
  /** Jump to the execute workbench for `local` or `local:<id>`. */
  onExecuteLocal?: (target: string) => void;
};

/**
 * Exec addresses the plain shell as `local` and every other environment as
 * `local:<id>`. The desktop catalog already returns those forms (`local`,
 * `local:zsh`, …); leave a prefixed id unchanged so it is not doubled.
 */
export function localExecTarget(id: string): string {
  if (id === "local" || id.startsWith("local:")) return id;
  return `local:${id}`;
}

function readExpanded(): boolean {
  try {
    return localStorage.getItem(EXPANDED_KEY) === "1";
  } catch {
    return false;
  }
}

function writeExpanded(expanded: boolean) {
  try {
    localStorage.setItem(EXPANDED_KEY, expanded ? "1" : "0");
  } catch {
    // Preference storage can be unavailable; the card still toggles.
  }
}

/**
 * The local-environment listing for Host Management. The built-in rows are
 * the shells of this machine's operating system — Windows consoles, the macOS
 * zsh/bash/sh set, or the Linux bash/sh/dash/zsh set — plus custom ones from
 * `~/.agent2ssh/environments.toml`. These are not SSH hosts and have no
 * connection step, so the card is an inventory, not a connector list.
 */
export default function LocalEnvironmentsCard({ onExecuteLocal }: Props) {
  const { t } = useI18n();
  const [environments, setEnvironments] = useState<LocalEnvInfo[]>([]);
  const [expanded, setExpanded] = useState(readExpanded);

  useEffect(() => {
    api
      .listLocalEnvironments()
      .then(setEnvironments)
      .catch(() => setEnvironments([]));
  }, []);

  if (environments.length === 0) return null;
  const platform = environments.find((env) => env.platform)?.platform;

  function toggleExpanded() {
    setExpanded((current) => {
      const next = !current;
      writeExpanded(next);
      return next;
    });
  }

  return (
    <Card className={expanded ? "space-y-3 p-4" : "p-2"}>
      <Button
        variant="ghost"
        align="start"
        block
        className="h-auto gap-2 px-2 py-1.5 font-semibold"
        aria-expanded={expanded}
        onClick={toggleExpanded}
      >
        {expanded ? (
          <ChevronDown size={16} className="text-muted-foreground" />
        ) : (
          <ChevronRight size={16} className="text-muted-foreground" />
        )}
        <Cpu size={16} className="text-muted-foreground" />
        {t("Local environments")}
        {platform && <Badge variant="outline">{platform}</Badge>}
        <Badge variant="secondary">
          {t("{count} discovered", { count: environments.length })}
        </Badge>
        <Badge variant="secondary" className="font-mono">
          local:*
        </Badge>
      </Button>
      {expanded && (
        <>
          <div className="grid gap-1.5 sm:grid-cols-2">
            {environments.map((env) => (
              <div
                key={env.id}
                className="rounded-lg border border-border px-3 py-2 text-xs"
              >
                <div className="flex items-center gap-2">
                  <span className="min-w-0 truncate font-mono font-semibold text-foreground">
                    {env.id}
                  </span>
                  {!env.builtin && (
                    <Badge variant="secondary" className="shrink-0">
                      {t("custom")}
                    </Badge>
                  )}
                  <Button
                    variant="outline"
                    size="sm"
                    className="ml-auto shrink-0 px-2"
                    onClick={() => onExecuteLocal?.(localExecTarget(env.id))}
                  >
                    <Play size={14} />
                    {t("Go to execute")}
                  </Button>
                </div>
                <div className="mt-0.5 flex min-w-0 flex-wrap items-center gap-x-2 text-[11px] text-muted-foreground">
                  {env.builtin ? (
                    <span className="truncate">{env.description}</span>
                  ) : (
                    <>
                      <span className="shrink-0">
                        {t("base")} {env.base}
                        {env.distro ? ` (${env.distro})` : ""}
                      </span>
                      {env.cwd && (
                        <span className="min-w-0 truncate font-mono" title={env.cwd}>
                          {env.cwd}
                        </span>
                      )}
                      {env.env_count > 0 && (
                        <span className="shrink-0">
                          {t("{count} env vars", { count: env.env_count })}
                        </span>
                      )}
                    </>
                  )}
                </div>
              </div>
            ))}
          </div>
          <div className="flex items-start gap-1.5 text-[11px] text-muted-foreground">
            <FileCode2 size={12} className="mt-0.5 shrink-0" />
            <span>
              {t(
                "These targets are the shells for this machine's operating system. Address them as local:<id> in Execution, the CLI and MCP. Custom environments are defined in ~/.agent2ssh/environments.toml (loaded at startup)."
              )}
            </span>
          </div>
        </>
      )}
    </Card>
  );
}
