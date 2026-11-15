import { Cpu, FileCode2 } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import { Badge } from "./ui/badge";
import { Card } from "./ui/card";

type LocalEnvInfo = {
  id: string;
  builtin: boolean;
  base: string | null;
  distro: string | null;
  cwd: string | null;
  env_count: number;
  description: string | null;
};

/**
 * The local-environment listing for Host Management: the built-in
 * `local:<id>` exec targets (pwsh / cmd / gitbash / vs / wsl / default) plus
 * the custom ones hand-defined in `~/.agent2ssh/environments.toml`. These are
 * not SSH hosts and have no connection step — exec addresses them directly —
 * so the card is an inventory, not a connector list.
 */
export default function LocalEnvironmentsCard() {
  const { t } = useI18n();
  const [environments, setEnvironments] = useState<LocalEnvInfo[]>([]);

  useEffect(() => {
    api
      .listLocalEnvironments()
      .then(setEnvironments)
      .catch(() => setEnvironments([]));
  }, []);

  if (environments.length === 0) return null;
  const custom = environments.filter((env) => !env.builtin);

  return (
    <Card className="space-y-3 p-4">
      <div className="flex items-center gap-2">
        <h3 className="flex items-center gap-2 font-semibold">
          <Cpu size={16} className="text-muted-foreground" />
          {t("Local environments")}
        </h3>
        <Badge variant="secondary" className="font-mono">
          local:*
        </Badge>
      </div>
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
            "Address them as local:<id> targets in Execution, the CLI and MCP. Custom environments are defined in ~/.agent2ssh/environments.toml (loaded at startup)."
          )}
        </span>
      </div>
    </Card>
  );
}
