import { ChevronDown, ChevronUp, Play, ShieldAlert } from "lucide-react";
import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import { labelCls } from "../lib/ui-classes";
import type { ExecResult, HostProfile, RiskLevel } from "../types";
import ApprovalDialog from "./ApprovalDialog";
import HostSelector from "./HostSelector";
import RiskBadge from "./RiskBadge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { Input } from "./ui/input";
import { Textarea } from "./ui/textarea";
import { useToast } from "./ui/toast";

type Props = {
  hosts: HostProfile[];
  initialHost?: string;
  /** Command carried in from outside (an approval jump). Empty values are ignored. */
  initialCommand?: string;
  /**
   * Bumps when the same `initialCommand` is sent again. A draft already in the
   * field is replaced — the incoming command wins, with no confirm dialog.
   */
  carryEpoch?: number;
  onExecComplete: () => void;
};

type LocalEnvInfo = {
  id: string;
  description: string | null;
};

function isLocalTarget(host: string): boolean {
  return host === "local" || host.startsWith("local:");
}

function growCommandField(el: HTMLTextAreaElement) {
  el.style.height = "auto";
  const next = el.scrollHeight;
  if (next > 0) el.style.height = `${next}px`;
}

function ExecPanel({ hosts, initialHost = "", initialCommand, carryEpoch = 0, onExecComplete }: Props) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [targetHost, setTargetHost] = useState(initialHost);
  const [command, setCommand] = useState(() =>
    initialCommand?.trim() ? initialCommand : "uname -a",
  );
  const commandRef = useRef<HTMLTextAreaElement>(null);
  const appliedCarry = useRef<string | undefined>(undefined);
  const focusValue = useRef<string | null>(null);
  const pendingToast = useRef<string | null>(null);
  const [force, setForce] = useState(false);
  const [result, setResult] = useState<ExecResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [pendingApproval, setPendingApproval] = useState(false);

  // Advanced options
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [stdin, setStdin] = useState("");
  const [timeoutSecs, setTimeoutSecs] = useState(60);
  const [maxOutputMb, setMaxOutputMb] = useState(4);

  // Pre-flight risk check
  const [previewRisk, setPreviewRisk] = useState<RiskLevel | null>(null);
  const [environments, setEnvironments] = useState<LocalEnvInfo[]>([]);

  useEffect(() => {
    let cancelled = false;
    api
      .listLocalEnvironments()
      .then((list) => {
        if (!cancelled) setEnvironments(list);
      })
      .catch(() => {
        if (!cancelled) setEnvironments([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const localGroups = useMemo(() => {
    const options = new Map<string, string>();
    const add = (id: string, label?: string | null) => {
      if (!id || options.has(id)) return;
      options.set(id, label && label !== id ? `${id} — ${label}` : id);
    };
    for (const env of environments) add(env.id, env.description);
    add("local");
    if (isLocalTarget(initialHost)) add(initialHost);
    if (isLocalTarget(targetHost)) add(targetHost);
    return [
      {
        label: t("Local environments"),
        options: [...options].map(([value, label]) => ({ value, label })),
      },
    ];
  }, [environments, initialHost, targetHost, t]);

  useLayoutEffect(() => {
    if (initialHost) setTargetHost(initialHost);
  }, [initialHost]);

  // Incoming approval/host jump replaces any unexecuted draft. No confirm —
  // the carried command wins, then the debounced risk preview below runs.
  useLayoutEffect(() => {
    if (!initialCommand?.trim()) return;
    const signature = `${carryEpoch}\0${initialCommand}`;
    if (appliedCarry.current === signature) return;
    appliedCarry.current = signature;
    focusValue.current = initialCommand;
    pendingToast.current = initialCommand;
    setCommand(initialCommand);
  }, [initialCommand, carryEpoch]);

  useLayoutEffect(() => {
    const el = commandRef.current;
    if (!el) return;
    growCommandField(el);
    const wanted = focusValue.current;
    if (wanted == null || el.value !== wanted) return;
    focusValue.current = null;
    el.focus();
    const end = el.value.length;
    el.setSelectionRange(end, end);
  }, [command]);

  useEffect(() => {
    const message = pendingToast.current;
    if (!message) return;
    pendingToast.current = null;
    showToast("success", t("Loaded pending command: {command}", { command: message }));
  }, [command, showToast, t]);

  useEffect(() => {
    // Reset result when target host changes
    setResult(null);
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
    setPendingApproval(false);
    try {
      const request = {
        host: targetHost,
        command,
        force: withForce || force,
        timeout_secs: timeoutSecs || null,
        stdin: stdin.trim() || null,
        max_output_bytes: maxOutputMb ? maxOutputMb * 1024 * 1024 : null,
      };
      const next = isLocalTarget(targetHost)
        ? await api.execLocal(request)
        : await api.execSshFull(request);
      setResult(next);
      onExecComplete();
    } catch (err) {
      showToast("error", String(err));
      reportError("exec-panel", "ssh exec failed", err);
    } finally {
      setBusy(false);
    }
  }

  async function handleRun() {
    if (!targetHost || !command.trim()) return;
    setBusy(true);
    try {
      const currentRisk = await api.classifyRisk(command, targetHost);
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
    <Card className="flex min-h-0 flex-1 flex-col gap-3.5 overflow-hidden p-4">
      <div className="flex items-center gap-2 font-semibold">
        <Play size={16} className="text-muted-foreground" />
        {t("Execute")}
        {previewRisk && <RiskBadge level={previewRisk} />}
      </div>
      <HostSelector
        hosts={hosts}
        value={targetHost}
        onChange={setTargetHost}
        disabled={busy}
        extraGroups={localGroups}
        hostGroupLabel={t("SSH hosts")}
      />
      <label className={labelCls}>
        {t("Command")}
        <Textarea
          ref={commandRef}
          rows={Math.max(2, command.split("\n").length)}
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          spellCheck={false}
          onKeyDown={(e) => {
            if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
              e.preventDefault();
              if (!busy && targetHost && command.trim()) handleRun();
            }
          }}
        />
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
      <Button onClick={handleRun} disabled={busy || !targetHost} block>
        <Play size={16} />
        {busy ? t("Running") : isLocalTarget(targetHost) ? t("Run locally") : t("Run over SSH")}
      </Button>

      <div className="min-h-[45vh] flex-1 overflow-auto rounded-md bg-canvas-0 text-foreground">
        {result ? (
          <>
            <div className="flex flex-wrap items-center gap-2.5 border-b border-line px-3.5 py-2.5 text-muted-foreground">
              exit={result.exit_code ?? "signal"} duration={result.duration_ms}ms{" "}
              <RiskBadge level={result.risk_level} />
              {result.truncated && (
                <span className="rounded bg-warning/80 px-1.5 py-px text-[11px] font-semibold text-canvas-0">
                  {t("output truncated")}
                </span>
              )}
            </div>
            <pre className="m-0 whitespace-pre-wrap break-words p-3.5 font-mono text-[13px]">
              {result.stdout || result.stderr || t("(no output)")}
            </pre>
            {result.stderr && (
              <pre className="m-0 whitespace-pre-wrap break-words border-t border-line p-3.5 font-mono text-[13px] text-risk-high">
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

export default memo(ExecPanel);
