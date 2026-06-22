import { ChevronDown, ChevronUp, Play, ShieldAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { ExecResult, HostProfile, RiskLevel } from "../types";
import ApprovalDialog from "./ApprovalDialog";
import HostSelector from "./HostSelector";
import RiskBadge from "./RiskBadge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { Input } from "./ui/input";
import { Textarea } from "./ui/textarea";

const labelCls = "grid gap-1.5 text-sm font-medium text-foreground/90";

type Props = {
  hosts: HostProfile[];
  initialHost?: string;
  onExecComplete: () => void;
};

export default function ExecPanel({ hosts, initialHost = "", onExecComplete }: Props) {
  const { t } = useI18n();
  const [targetHost, setTargetHost] = useState(initialHost);
  const [command, setCommand] = useState("uname -a");
  const [force, setForce] = useState(false);
  const [result, setResult] = useState<ExecResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pendingApproval, setPendingApproval] = useState(false);

  // Advanced options
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [stdin, setStdin] = useState("");
  const [timeoutSecs, setTimeoutSecs] = useState(60);
  const [maxOutputMb, setMaxOutputMb] = useState(4);

  // Pre-flight risk check
  const [previewRisk, setPreviewRisk] = useState<RiskLevel | null>(null);

  useEffect(() => {
    // Reset result when target host changes
    setResult(null);
    setError(null);
    setPendingApproval(false);
    setPreviewRisk(null);
  }, [targetHost]);

  // Debounced risk preview
  useEffect(() => {
    if (!command.trim() || !targetHost) {
      setPreviewRisk(null);
      return;
    }
    const timer = setTimeout(async () => {
      try {
        const level = await api.classifyRisk(command, targetHost);
        setPreviewRisk(level);
      } catch {
        setPreviewRisk(null);
      }
    }, 300);
    return () => clearTimeout(timer);
  }, [command, targetHost]);

  async function runCommand(withForce = false) {
    if (!targetHost || !command.trim()) return;
    setBusy(true);
    setError(null);
    setPendingApproval(false);
    try {
      const next = await api.execSshFull({
        host: targetHost,
        command,
        force: withForce || force,
        timeout_secs: timeoutSecs || null,
        stdin: stdin.trim() || null,
        max_output_bytes: maxOutputMb ? maxOutputMb * 1024 * 1024 : null,
      });
      setResult(next);
      onExecComplete();
    } catch (err) {
      setError(String(err));
      reportError("exec-panel", "ssh exec failed", err);
    } finally {
      setBusy(false);
    }
  }

  async function handleRun() {
    if (!targetHost || !command.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const currentRisk = await api.classifyRisk(command, targetHost);
      setPreviewRisk(currentRisk);
      if (currentRisk === "blocked") {
        setError(t("This command is blocked (risk=blocked)."));
        return;
      }
      if (currentRisk === "high" && !force) {
        setPendingApproval(true);
        return;
      }
    } catch (err) {
      reportError("exec-panel", "risk check failed before run", err);
    } finally {
      setBusy(false);
    }

    runCommand();
  }

  function handleApprovalConfirm() {
    setPendingApproval(false);
    setForce(true);
    runCommand(true);
  }

  function handleApprovalCancel() {
    setPendingApproval(false);
  }

  return (
    <Card className="space-y-3.5 p-4">
      <div className="flex items-center gap-2 font-semibold">
        <Play size={16} className="text-muted-foreground" />
        {t("Execute")}
        {previewRisk && <RiskBadge level={previewRisk} />}
      </div>
      {error && (
        <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2.5 text-sm text-destructive">
          {error}
        </div>
      )}
      <HostSelector hosts={hosts} value={targetHost} onChange={setTargetHost} disabled={busy} />
      <label className={labelCls}>
        {t("Command")}
        <Textarea value={command} onChange={(e) => setCommand(e.target.value)} spellCheck={false} />
      </label>

      <button
        type="button"
        className="inline-flex items-center gap-1.5 text-sm font-semibold text-primary"
        onClick={() => setShowAdvanced(!showAdvanced)}
      >
        {showAdvanced ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
        {t("Advanced options")}
      </button>

      {showAdvanced && (
        <div className="grid gap-3 rounded-lg border border-border bg-muted/40 p-3">
          <label className={labelCls}>
            {t("Stdin (piped to command)")}
            <Textarea
              className="min-h-16"
              value={stdin}
              onChange={(e) => setStdin(e.target.value)}
              placeholder={t("optional input...")}
            />
          </label>
          <div className="grid grid-cols-2 gap-2.5">
            <label className={labelCls}>
              {t("Timeout (seconds)")}
              <Input
                type="number"
                min={1}
                max={3600}
                value={timeoutSecs}
                onChange={(e) => setTimeoutSecs(Number(e.target.value))}
              />
            </label>
            <label className={labelCls}>
              {t("Max output (MiB)")}
              <Input
                type="number"
                min={1}
                max={64}
                value={maxOutputMb}
                onChange={(e) => setMaxOutputMb(Number(e.target.value))}
              />
            </label>
          </div>
        </div>
      )}

      <label className="flex cursor-pointer select-none items-center gap-2 text-sm font-semibold text-destructive">
        <input
          type="checkbox"
          className="size-4 accent-destructive"
          checked={force}
          onChange={(e) => setForce(e.target.checked)}
        />
        <ShieldAlert size={14} />
        {t("Force (allow high-risk commands)")}
      </label>
      <Button onClick={handleRun} disabled={busy || !targetHost} className="w-full">
        <Play size={16} />
        {busy ? t("Running") : t("Run over SSH")}
      </Button>

      <div className="min-h-[250px] overflow-auto rounded-md bg-[#0e1620] text-[#e6edf3]">
        {result ? (
          <>
            <div className="flex flex-wrap items-center gap-2.5 border-b border-white/10 px-3.5 py-2.5 text-[#8fb0c5]">
              exit={result.exit_code ?? "signal"} duration={result.duration_ms}ms{" "}
              <RiskBadge level={result.risk_level} />
              {result.truncated && (
                <span className="rounded bg-warning/80 px-1.5 py-px text-[11px] font-semibold text-[#fef3c7]">
                  {t("output truncated")}
                </span>
              )}
            </div>
            <pre className="m-0 whitespace-pre-wrap break-words p-3.5 font-mono text-[13px]">
              {result.stdout || result.stderr || t("(no output)")}
            </pre>
            {result.stderr && (
              <pre className="m-0 whitespace-pre-wrap break-words border-t border-white/10 p-3.5 font-mono text-[13px] text-[#ffb4a6]">
                {result.stderr}
              </pre>
            )}
          </>
        ) : (
          <pre className="m-0 whitespace-pre-wrap break-words p-3.5 font-mono text-[13px]">
            {t("Command output will appear here.")}
          </pre>
        )}
      </div>

      {pendingApproval && (
        <ApprovalDialog
          command={command}
          riskLevel="high"
          onConfirm={handleApprovalConfirm}
          onCancel={handleApprovalCancel}
        />
      )}
    </Card>
  );
}
