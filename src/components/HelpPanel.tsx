import { BookOpen, CheckCircle2, Clipboard, FileText, ShieldCheck, Terminal } from "lucide-react";
import { useState } from "react";
import { useI18n } from "../i18n";
import { cn } from "../lib/utils";
import { Button } from "./ui/button";
import { Card } from "./ui/card";

const quickCommands = [
  "agent2ssh host import-config",
  "agent2ssh host list",
  'agent2ssh exec mybox "hostname && uptime"',
  "agent2ssh daemon start",
  "agent2ssh status",
];

const docs = [
  { title: "Help overview", path: "docs/guides/help.md" },
  { title: "CLI quickstart", path: "docs/guides/cli-quickstart.md" },
  { title: "MCP quickstart", path: "docs/guides/mcp-quickstart.md" },
  { title: "Configuration guide", path: "docs/guides/configuration-guide.md" },
  { title: "Web console guide", path: "docs/guides/web-console-guide.md" },
  { title: "Daemon API quickstart", path: "docs/guides/daemon-api-quickstart.md" },
  { title: "MCP client templates", path: "docs/guides/mcp-client-templates.md" },
];

export default function HelpPanel() {
  const { t } = useI18n();
  const [copied, setCopied] = useState<string | null>(null);

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(text);
      window.setTimeout(() => setCopied(null), 1600);
    } catch {
      setCopied(null);
    }
  }

  return (
    <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_minmax(320px,0.42fr)]">
      <Card className="space-y-5 p-4">
        <div className="flex items-start gap-3">
          <div className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
            <BookOpen size={18} />
          </div>
          <div className="min-w-0">
            <h3 className="text-base font-semibold">{t("Agent2SSH Help")}</h3>
            <p className="mt-1 text-sm leading-6 text-muted-foreground">
              {t(
                "Use Agent2SSH as a local SSH capability layer for desktop workflows, CLI automation, and MCP agents."
              )}
            </p>
          </div>
        </div>

        <section className="grid gap-3">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <CheckCircle2 size={16} className="text-success" />
            {t("First run checklist")}
          </div>
          <div className="grid gap-2 md:grid-cols-2">
            {[
              "Import or add a host profile.",
              "Run a low-risk command such as hostname or uptime.",
              "Start the local daemon before using sessions, tunnels, or MCP.",
              "Use the execution gate when you need a local kill switch.",
            ].map((item) => (
              <div
                key={item}
                className="rounded-lg border border-border bg-muted/30 px-3 py-2.5 text-sm text-foreground/85"
              >
                {t(item)}
              </div>
            ))}
          </div>
        </section>

        <section className="grid gap-3">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <Terminal size={16} className="text-muted-foreground" />
            {t("Useful commands")}
          </div>
          <div className="overflow-hidden rounded-lg border border-border">
            {quickCommands.map((command) => (
              <div
                key={command}
                className="flex items-center gap-2 border-b border-border bg-card px-3 py-2 last:border-b-0"
              >
                <code className="min-w-0 flex-1 truncate font-mono text-xs text-foreground">
                  {command}
                </code>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => copy(command)}
                  className={cn("h-7 px-2 text-xs", copied === command && "text-success")}
                  title={t("Copy command")}
                >
                  <Clipboard size={13} />
                  {copied === command ? t("Copied") : t("Copy")}
                </Button>
              </div>
            ))}
          </div>
        </section>

        <section className="grid gap-3">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <ShieldCheck size={16} className="text-success" />
            {t("Safety model")}
          </div>
          <div className="grid gap-2 text-sm leading-6 text-muted-foreground">
            <p>
              {t(
                "High-risk commands require explicit approval or force, while blocked commands cannot be bypassed."
              )}
            </p>
            <p>
              {t(
                "SSH host fingerprints are trusted automatically on first use and blocked if they change later."
              )}
            </p>
            <p>
              {t("Local runtime data is stored under ~/.agent2ssh/. Do not share that directory.")}
            </p>
          </div>
        </section>
      </Card>

      <Card className="space-y-4 p-4">
        <div className="flex items-center gap-2 text-sm font-semibold">
          <FileText size={16} className="text-muted-foreground" />
          {t("Documentation index")}
        </div>
        <div className="grid gap-2">
          {docs.map((doc) => (
            <div key={doc.path} className="rounded-lg border border-border bg-muted/25 px-3 py-2.5">
              <div className="text-sm font-medium">{t(doc.title)}</div>
              <code className="mt-1 block truncate font-mono text-xs text-muted-foreground">
                {doc.path}
              </code>
            </div>
          ))}
        </div>
        <div className="rounded-lg border border-border bg-card px-3 py-2.5 text-sm leading-6 text-muted-foreground">
          {t(
            "These files are bundled in the repository and can be opened from the project directory."
          )}
        </div>
      </Card>
    </div>
  );
}
