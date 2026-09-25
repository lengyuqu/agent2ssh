import { Boxes, Check, Copy, RefreshCw, SquareTerminal } from "lucide-react";
import { useEffect, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { ContainerDiscoveryTarget, ContainerPlatform, ContainerRuntimeInfo } from "../types";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { IconButton } from "./ui/icon-button";
import { EmptyState, LoadingState, Spinner } from "./ui/state";
import { useToast } from "./ui/toast";

/**
 * Product names, not copy: Docker, wslc and Kubernetes are what the user types
 * at a shell, so they are not translated.
 */
const PLATFORM_LABEL: Record<ContainerPlatform, string> = {
  docker: "Docker",
  wslc: "wslc",
  apple: "Apple container",
  k8s: "Kubernetes",
};

/**
 * Quote an argv part for display and copying.
 *
 * Display only: the "open in terminal" path passes the argv itself, so nothing
 * here is ever parsed back into a command. Double quotes are used because the
 * part that needs them in practice is a Windows install path.
 */
function quotePart(part: string): string {
  return /\s/.test(part) ? `"${part}"` : part;
}

/** The exec command for a target, as a user would type it. */
function execCommand(target: ContainerDiscoveryTarget): string {
  return [target.exec_binary, ...target.exec_args].map(quotePart).join(" ");
}

/**
 * What this host can say about one runtime: installed and chosen, installed,
 * absent, or impossible here. "Unsupported" and "not installed" are kept apart
 * because they call for different actions — one needs a different machine, the
 * other needs an install.
 */
function runtimeState(
  info: ContainerRuntimeInfo,
  t: (text: string, vars?: Record<string, string | number>) => string
): string {
  if (info.available) {
    return info.selected ? t("detected (auto)") : t("detected");
  }
  return info.supported ? t("not installed") : t("unsupported here");
}

export default function ContainersPanel() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [targets, setTargets] = useState<ContainerDiscoveryTarget[]>([]);
  const [runtimes, setRuntimes] = useState<ContainerRuntimeInfo[]>([]);
  const [includeStopped, setIncludeStopped] = useState(false);
  const [busy, setBusy] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const [launching, setLaunching] = useState<string | null>(null);

  async function refresh(includeStoppedNow = includeStopped) {
    setBusy(true);
    try {
      // Both calls together: the list answers "what can I connect to", the
      // runtime states answer "why is it empty" when it is.
      const [found, infos] = await Promise.all([
        api.discoverContainers({ all: includeStoppedNow }),
        api.containerRuntimes(),
      ]);
      setTargets(found);
      setRuntimes(infos);
    } catch (err) {
      showToast("error", String(err));
      reportError("containers-panel", "container discovery failed", err);
    } finally {
      setBusy(false);
      setLoaded(true);
    }
  }

  useEffect(() => {
    refresh(false);
    // Mount only, matching the other panels: the refresh button owns re-reads.
  }, []);

  function toggleIncludeStopped() {
    const next = !includeStopped;
    setIncludeStopped(next);
    refresh(next);
  }

  function copyCommand(target: ContainerDiscoveryTarget) {
    navigator.clipboard.writeText(execCommand(target));
    setCopied(target.id);
    setTimeout(() => setCopied((current) => (current === target.id ? null : current)), 2000);
  }

  async function openInTerminal(target: ContainerDiscoveryTarget) {
    setLaunching(target.id);
    try {
      await api.launchContainerTarget(target.exec_binary, target.exec_args);
    } catch (err) {
      showToast("error", String(err));
      reportError("containers-panel", "launch container terminal failed", err);
    } finally {
      setLaunching(null);
    }
  }

  const anyAvailable = runtimes.some((info) => info.available);

  return (
    <Card className="space-y-3.5 p-4">
      <div className="flex items-center gap-2">
        <h3 className="flex items-center gap-2 font-semibold">
          <Boxes size={16} className="text-muted-foreground" />
          {t("Containers")}
        </h3>
        {targets.length > 0 && (
          <span className="text-xs text-muted-foreground">
            {t("{count} targets", { count: targets.length })}
          </span>
        )}
        <Button
          variant="secondary"
          size="sm"
          className="ml-auto"
          onClick={toggleIncludeStopped}
          disabled={busy}
          title={t("Include stopped containers")}
        >
          {includeStopped ? <Check size={14} /> : null}
          {t("Include stopped")}
        </Button>
        <IconButton
          onClick={() => refresh()}
          disabled={busy}
          aria-label={t("Refresh containers")}
          title={t("Refresh containers")}
        >
          {busy ? <Spinner size={14} /> : <RefreshCw size={14} />}
        </IconButton>
      </div>

      {runtimes.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5">
          {runtimes.map((info) => (
            <Badge
              key={info.runtime}
              variant={info.available ? (info.selected ? "success" : "secondary") : "outline"}
              title={info.unsupported_reason ?? info.binary ?? undefined}
            >
              {/* Separate spans so each half is its own text node: the runtime
                  name and its state answer different questions, and a test (or a
                  screen reader stopping mid-element) should not have to match
                  them glued together. */}
              <span>{info.display_name}</span>
              <span className="opacity-75">{runtimeState(info, t)}</span>
            </Badge>
          ))}
        </div>
      )}

      {!loaded && busy && <LoadingState label={t("Reading container runtimes...")} />}

      {loaded && targets.length === 0 && (
        <EmptyState
          icon={Boxes}
          title={anyAvailable ? t("No containers found") : t("No container runtime is available on this host")}
          description={
            anyAvailable
              ? t("Start a container, then refresh — or include stopped ones.")
              : t(
                  "Install Docker, WSL containers (wslc) or Apple container on this host, then refresh. Only the runtimes this platform supports are listed."
                )
          }
        />
      )}

      <div className="grid gap-2">
        {targets.map((target) => (
          <div
            key={target.id}
            className="flex flex-wrap items-center gap-2 rounded-md border border-border bg-muted/20 px-2.5 py-2"
          >
            <Badge variant="outline">{PLATFORM_LABEL[target.platform]}</Badge>
            <div className="min-w-0 flex-1">
              <div className="truncate text-sm font-medium text-foreground" title={target.container_name}>
                {target.container_name}
              </div>
              <div className="truncate text-xs text-muted-foreground" title={execCommand(target)}>
                {target.image ? `${target.image} · ` : ""}
                {target.status}
                {target.namespace ? ` · ${target.namespace}` : ""}
              </div>
            </div>
            <code className="hidden max-w-[18rem] truncate rounded bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground lg:block">
              {execCommand(target)}
            </code>
            <IconButton
              variant="ghost"
              size="sm"
              onClick={() => copyCommand(target)}
              aria-label={t("Copy exec command")}
              title={t("Copy exec command")}
            >
              {copied === target.id ? <Check size={14} /> : <Copy size={14} />}
            </IconButton>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => openInTerminal(target)}
              disabled={launching === target.id}
              title={t("Open in terminal")}
            >
              <SquareTerminal size={14} />
              {launching === target.id ? t("Opening...") : t("Open in terminal")}
            </Button>
          </div>
        ))}
      </div>
    </Card>
  );
}
