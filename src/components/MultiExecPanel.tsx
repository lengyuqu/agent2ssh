import { Play, Server } from "lucide-react";
import { useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { ExecMultiResult, HostProfile } from "../types";
import HostDiffView from "./HostDiffView";
import RiskBadge from "./RiskBadge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { Input } from "./ui/input";
import { Textarea } from "./ui/textarea";
import { useToast } from "./ui/toast";
import { cn } from "../lib/utils";

const labelCls = "grid gap-1.5 text-sm font-medium text-foreground/90";

type Props = {
  hosts: HostProfile[];
  onExecComplete: () => void;
};

export default function MultiExecPanel({ hosts, onExecComplete }: Props) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [selected, setSelected] = useState<string[]>([]);
  const [command, setCommand] = useState("");
  const [force, setForce] = useState(false);
  const [tags, setTags] = useState("");
  const [results, setResults] = useState<ExecMultiResult[]>([]);
  const [busy, setBusy] = useState(false);

  function toggleHost(name: string) {
    setSelected((prev) =>
      prev.includes(name) ? prev.filter((n) => n !== name) : [...prev, name]
    );
  }

  function selectAll() {
    if (selected.length === hosts.length) {
      setSelected([]);
    } else {
      setSelected(hosts.map((h) => h.name));
    }
  }

  async function runMulti() {
    if (selected.length === 0 || !command.trim()) return;
    setBusy(true);
    try {
      const parsedTags = tags.trim() ? tags.split(",").map((t) => t.trim()) : undefined;
      const res = await api.execMulti(selected, command, force, undefined, parsedTags);
      setResults(res);
      onExecComplete();
    } catch (err) {
      showToast("error", String(err));
      reportError("multi-exec-panel", "multi-host exec failed", err);
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card className="space-y-3 p-4">
      <div className="flex items-center gap-2 font-semibold">
        <Server size={16} className="text-muted-foreground" />
        {t("Multi-host Execute")}
      </div>

      <div className="space-y-2">
        <button
          type="button"
          className="text-sm font-semibold text-primary"
          onClick={selectAll}
        >
          {selected.length === hosts.length ? t("Deselect all") : t("Select all")}
        </button>
        <div className="flex flex-wrap gap-1.5">
          {hosts.map((h) => (
            <button
              key={h.name}
              className={cn(
                "rounded-full border px-3 py-1 text-sm font-semibold transition-colors",
                selected.includes(h.name)
                  ? "border-primary bg-primary text-primary-foreground"
                  : "border-border bg-card text-muted-foreground hover:bg-muted"
              )}
              onClick={() => toggleHost(h.name)}
            >
              {h.name}
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
              if (!busy && selected.length > 0 && command.trim()) runMulti();
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
      <Button
        onClick={runMulti}
        disabled={busy || selected.length === 0 || !command.trim()}
        className="w-full"
      >
        <Play size={14} />
        {busy
          ? t("Running on {count} hosts...", { count: selected.length })
          : t(selected.length === 1 ? "Run on {count} host" : "Run on {count} hosts", {
              count: selected.length,
            })}
      </Button>

      {results.length > 0 && (
        <div className="space-y-2">
          {results.map((r) => (
            <div
              key={r.host}
              className="flex items-center gap-3 rounded-md border border-border bg-muted/40 px-2.5 py-2"
            >
              <strong className="font-semibold">{r.host}</strong>
              {r.result ? (
                <>
                  <span className="text-sm text-muted-foreground">
                    exit={r.result.exit_code ?? "signal"} {r.result.duration_ms}ms
                  </span>
                  <RiskBadge level={r.result.risk_level} />
                </>
              ) : (
                <span className="text-sm text-destructive">{r.error}</span>
              )}
            </div>
          ))}
          {results.map((r) =>
            r.result ? (
              <div key={`${r.host}-output`} className="overflow-hidden rounded-md bg-[#0e1620]">
                <div className="bg-white/5 px-3.5 py-1.5 text-xs font-semibold text-[#8fb0c5]">
                  {r.host}
                </div>
                <pre className="m-0 whitespace-pre-wrap break-words px-3.5 py-2.5 font-mono text-[13px] text-[#e6edf3]">
                  {r.result.stdout || r.result.stderr || t("(no output)")}
                </pre>
              </div>
            ) : null
          )}
        </div>
      )}

      {results.length > 0 && <HostDiffView results={results} />}
    </Card>
  );
}
