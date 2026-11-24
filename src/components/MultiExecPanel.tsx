import { Play, Server } from "lucide-react";
import { memo, useEffect, useMemo, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import { labelCls } from "../lib/ui-classes";
import { cn } from "../lib/utils";
import type { ExecMultiResult, HostProfile, RiskLevel } from "../types";
import ApprovalDialog from "./ApprovalDialog";
import HostDiffView from "./HostDiffView";
import RiskBadge from "./RiskBadge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { Input } from "./ui/input";
import { Textarea } from "./ui/textarea";
import { useToast } from "./ui/toast";

type Props = {
  hosts: HostProfile[];
  onExecComplete: () => void;
};

type ResultView = "all" | "failed" | "diff";

function parseTagList(raw: string): string[] {
  return raw
    .split(",")
    .map((part) => part.trim())
    .filter((part) => part.length > 0);
}

function hostsMatchingTags(hosts: HostProfile[], tagList: string[]): string[] {
  if (tagList.length === 0) return [];
  return hosts
    .filter((host) => (host.tags ?? []).some((tag) => tagList.includes(tag)))
    .map((host) => host.name);
}

function isFailed(result: ExecMultiResult): boolean {
  if (!result.result) return true;
  return result.result.exit_code !== 0;
}

function unique(values: string[]): string[] {
  return [...new Set(values)];
}

function MultiExecPanel({ hosts, onExecComplete }: Props) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [selected, setSelected] = useState<string[]>([]);
  const [command, setCommand] = useState("");
  const [force, setForce] = useState(false);
  const [tags, setTags] = useState("");
  const [results, setResults] = useState<ExecMultiResult[]>([]);
  const [busy, setBusy] = useState(false);
  const [pendingApproval, setPendingApproval] = useState(false);
  const [previewRisk, setPreviewRisk] = useState<RiskLevel | null>(null);
  const [resultView, setResultView] = useState<ResultView>("all");
  const [activeResult, setActiveResult] = useState("");

  const tagList = useMemo(() => parseTagList(tags), [tags]);
  const effectiveTargets = useMemo(
    () => unique([...selected, ...hostsMatchingTags(hosts, tagList)]),
    [hosts, selected, tagList],
  );
  const riskHost = effectiveTargets[0] ?? "";
  const groups = useMemo(() => unique(hosts.map((host) => host.group).filter(Boolean)), [hosts]);
  const knownTags = useMemo(
    () => unique(hosts.flatMap((host) => host.tags ?? [])),
    [hosts],
  );

  const failedResults = useMemo(() => results.filter(isFailed), [results]);
  const listed = resultView === "failed" ? failedResults : results;
  const shown = listed.find((result) => result.host === activeResult) ?? listed[0] ?? null;

  useEffect(() => {
    if (results.length === 0) {
      setActiveResult("");
      return;
    }
    const firstFailed = results.find(isFailed);
    setActiveResult((firstFailed ?? results[0]).host);
    setResultView("all");
  }, [results]);

  useEffect(() => {
    if (!command.trim() || !riskHost) {
      setPreviewRisk(null);
      return;
    }
    const timer = setTimeout(async () => {
      try {
        const level = await api.classifyRisk(command, riskHost);
        setPreviewRisk(level);
      } catch {
        setPreviewRisk(null);
      }
    }, 300);
    return () => clearTimeout(timer);
  }, [command, riskHost]);

  function toggleHost(name: string) {
    setSelected((prev) =>
      prev.includes(name) ? prev.filter((item) => item !== name) : [...prev, name],
    );
  }

  function toggleNames(names: string[]) {
    setSelected((prev) => {
      const allOn = names.length > 0 && names.every((name) => prev.includes(name));
      if (allOn) return prev.filter((name) => !names.includes(name));
      return unique([...prev, ...names]);
    });
  }

  function selectAll() {
    if (selected.length === hosts.length) {
      setSelected([]);
    } else {
      setSelected(hosts.map((host) => host.name));
    }
  }

  async function runMulti(withForce = false) {
    if (effectiveTargets.length === 0 || !command.trim()) return;
    setBusy(true);
    try {
      const parsedTags = tagList.length > 0 ? tagList : undefined;
      const res = await api.execMulti(
        selected,
        command,
        withForce || force,
        undefined,
        parsedTags,
      );
      setResults(res);
      onExecComplete();
    } catch (err) {
      showToast("error", String(err));
      reportError("multi-exec-panel", "multi-host exec failed", err);
    } finally {
      setBusy(false);
    }
  }

  async function handleRun() {
    if (effectiveTargets.length === 0 || !command.trim()) return;
    setBusy(true);
    try {
      const currentRisk = await api.classifyRisk(command, riskHost);
      setPreviewRisk(currentRisk);
      if (currentRisk === "blocked") {
        showToast("error", t("This command is blocked (risk=blocked)."));
        return;
      }
      if (currentRisk === "high" && !force) {
        setPendingApproval(true);
        return;
      }
    } catch (err) {
      reportError("multi-exec-panel", "risk check failed before run", err);
    } finally {
      setBusy(false);
    }

    void runMulti();
  }

  function handleApprovalConfirm() {
    setPendingApproval(false);
    setForce(true);
    void runMulti(true);
  }

  const runCount = effectiveTargets.length;
  const canRun = !busy && runCount > 0 && command.trim().length > 0;

  return (
    <Card className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden p-4">
      <div className="flex items-center gap-2 font-semibold">
        <Server size={16} className="text-muted-foreground" />
        {t("Multi-host Execute")}
        {previewRisk && <RiskBadge level={previewRisk} />}
      </div>

      <div className="space-y-2">
        <button type="button" className="text-sm font-semibold text-primary" onClick={selectAll}>
          {selected.length === hosts.length ? t("Deselect all") : t("Select all")}
        </button>
        {groups.length > 0 && (
          <div role="group" aria-label={t("Groups")} className="flex flex-wrap items-center gap-1.5">
            <span className="text-xs font-semibold text-muted-foreground">{t("Groups")}</span>
            {groups.map((group) => {
              const names = hosts.filter((host) => host.group === group).map((host) => host.name);
              const on = names.length > 0 && names.every((name) => selected.includes(name));
              return (
                <button
                  key={group}
                  type="button"
                  aria-pressed={on}
                  className={cn(
                    "rounded-full border px-3 py-1 text-sm font-semibold transition-colors",
                    on
                      ? "border-primary bg-primary text-primary-foreground"
                      : "border-border bg-card text-muted-foreground hover:bg-muted",
                  )}
                  onClick={() => toggleNames(names)}
                >
                  {group}
                </button>
              );
            })}
          </div>
        )}
        {knownTags.length > 0 && (
          <div role="group" aria-label={t("Tags")} className="flex flex-wrap items-center gap-1.5">
            <span className="text-xs font-semibold text-muted-foreground">{t("Tags")}</span>
            {knownTags.map((tag) => {
              const names = hosts
                .filter((host) => (host.tags ?? []).includes(tag))
                .map((host) => host.name);
              const on = names.length > 0 && names.every((name) => selected.includes(name));
              return (
                <button
                  key={tag}
                  type="button"
                  aria-pressed={on}
                  className={cn(
                    "rounded-full border px-3 py-1 text-sm font-semibold transition-colors",
                    on
                      ? "border-primary bg-primary text-primary-foreground"
                      : "border-border bg-card text-muted-foreground hover:bg-muted",
                  )}
                  onClick={() => toggleNames(names)}
                >
                  {tag}
                </button>
              );
            })}
          </div>
        )}
        <div role="group" aria-label={t("Hosts")} className="flex flex-wrap gap-1.5">
          {hosts.map((host) => (
            <button
              key={host.name}
              type="button"
              aria-pressed={selected.includes(host.name)}
              className={cn(
                "rounded-full border px-3 py-1 text-sm font-semibold transition-colors",
                selected.includes(host.name)
                  ? "border-primary bg-primary text-primary-foreground"
                  : "border-border bg-card text-muted-foreground hover:bg-muted",
              )}
              onClick={() => toggleHost(host.name)}
            >
              {host.name}
            </button>
          ))}
          {hosts.length === 0 && (
            <span className="text-sm text-muted-foreground">{t("No hosts")}</span>
          )}
        </div>
      </div>

      <label className={labelCls}>
        {t("Command")}
        <Textarea
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          spellCheck={false}
          placeholder="uname -a"
          onKeyDown={(e) => {
            if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
              e.preventDefault();
              if (canRun) void handleRun();
            }
          }}
        />
      </label>
      <label className="flex cursor-pointer select-none items-center gap-2 text-sm font-semibold text-destructive">
        <input
          type="checkbox"
          className="size-4 accent-destructive"
          checked={force}
          onChange={(e) => setForce(e.target.checked)}
        />
        {t("Force")}
      </label>
      <label className={labelCls}>
        {t("Tags (comma-separated, optional)")}
        <Input
          value={tags}
          onChange={(e) => setTags(e.target.value)}
          placeholder="web, production"
        />
      </label>
      <p className="text-sm text-muted-foreground">
        {t(runCount === 1 ? "{count} host will run" : "{count} hosts will run", {
          count: runCount,
        })}
      </p>
      <Button onClick={() => void handleRun()} disabled={!canRun} block>
        <Play size={14} />
        {busy
          ? t("Running on {count} hosts...", { count: runCount })
          : t(runCount === 1 ? "Run on {count} host" : "Run on {count} hosts", {
              count: runCount,
            })}
      </Button>

      {results.length > 0 && (
        <div className="flex min-h-0 flex-1 flex-col gap-2">
          <div role="tablist" aria-label={t("Results")} className="flex flex-wrap gap-1.5">
            {(
              [
                ["all", t("All ({count})", { count: results.length })],
                ["failed", t("Failed only ({count})", { count: failedResults.length })],
                ["diff", t("Output diff")],
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                type="button"
                role="tab"
                aria-selected={resultView === id}
                className={cn(
                  "rounded-full px-3 py-1 text-sm font-semibold transition-colors",
                  resultView === id
                    ? "bg-primary text-primary-foreground"
                    : "text-muted-foreground hover:text-foreground",
                )}
                onClick={() => setResultView(id)}
              >
                {label}
              </button>
            ))}
          </div>
          {resultView === "diff" ? (
            <div className="min-h-0 flex-1 overflow-auto">
              <HostDiffView results={results} />
            </div>
          ) : (
            <div className="flex min-h-0 flex-1 gap-3 overflow-hidden">
              <div
                role="listbox"
                aria-label={t("Results")}
                className="flex w-56 shrink-0 flex-col gap-1 overflow-auto"
              >
                {listed.map((result) => {
                  const failed = isFailed(result);
                  const active = shown?.host === result.host;
                  return (
                    <button
                      key={result.host}
                      type="button"
                      role="option"
                      aria-selected={active}
                      className={cn(
                        "flex items-center gap-2 rounded-md border px-2.5 py-2 text-left text-sm",
                        active ? "border-primary bg-primary/10" : "border-border bg-card",
                      )}
                      onClick={() => setActiveResult(result.host)}
                    >
                      <span
                        className={cn(
                          "size-2 shrink-0 rounded-full",
                          failed ? "bg-destructive" : "bg-success",
                        )}
                      />
                      <span className="min-w-0 flex-1 truncate font-semibold">{result.host}</span>
                      <span className="shrink-0 text-xs text-muted-foreground">
                        {result.result
                          ? `exit=${result.result.exit_code ?? "signal"} ${result.result.duration_ms}ms`
                          : result.error}
                      </span>
                    </button>
                  );
                })}
              </div>
              <div className="min-h-0 min-w-0 flex-1 overflow-auto rounded-md bg-canvas-0 text-foreground">
                <pre
                  className={cn(
                    "m-0 whitespace-pre-wrap break-words p-3.5 font-mono text-[13px]",
                    shown?.result && !shown.result.stdout && shown.result.stderr && "text-risk-high",
                  )}
                >
                  {shown?.result
                    ? shown.result.stdout || shown.result.stderr || t("(no output)")
                    : (shown?.error ?? "")}
                </pre>
                {shown?.result?.stdout && shown.result.stderr && (
                  <pre className="m-0 whitespace-pre-wrap break-words border-t border-line p-3.5 font-mono text-[13px] text-risk-high">
                    {shown.result.stderr}
                  </pre>
                )}
              </div>
            </div>
          )}
        </div>
      )}

      {pendingApproval && (
        <ApprovalDialog
          command={command}
          riskLevel="high"
          onConfirm={handleApprovalConfirm}
          onCancel={() => setPendingApproval(false)}
        />
      )}
    </Card>
  );
}

export default memo(MultiExecPanel);
