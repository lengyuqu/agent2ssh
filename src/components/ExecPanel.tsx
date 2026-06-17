import { ChevronDown, ChevronUp, Play, ShieldAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import type { ExecResult, RiskLevel } from "../types";
import ApprovalDialog from "./ApprovalDialog";
import RiskBadge from "./RiskBadge";

type Props = {
  selectedHost: string;
  onExecComplete: () => void;
};

export default function ExecPanel({ selectedHost, onExecComplete }: Props) {
  const { t } = useI18n();
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
    // Reset result when host changes
    setResult(null);
    setError(null);
    setPendingApproval(false);
    setPreviewRisk(null);
  }, [selectedHost]);

  // Debounced risk preview
  useEffect(() => {
    if (!command.trim()) {
      setPreviewRisk(null);
      return;
    }
    const timer = setTimeout(async () => {
      try {
        const level = await api.classifyRisk(command);
        setPreviewRisk(level);
      } catch {
        setPreviewRisk(null);
      }
    }, 300);
    return () => clearTimeout(timer);
  }, [command]);

  async function runCommand(withForce = false) {
    if (!selectedHost || !command.trim()) return;
    setBusy(true);
    setError(null);
    setPendingApproval(false);
    try {
      const next = await api.execSshFull({
        host: selectedHost,
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
    } finally {
      setBusy(false);
    }
  }

  function handleRun() {
    if (previewRisk === "high" && !force) {
      setPendingApproval(true);
      return;
    }
    if (previewRisk === "blocked") {
      setError(t("This command is blocked (risk=blocked)."));
      return;
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
    <section className="panel command-panel">
      <div className="panel-title">
        <Play size={16} />
        {t("Execute")}
        {previewRisk && <RiskBadge level={previewRisk} />}
      </div>
      {error && <div className="error">{error}</div>}
      <label>
        {t("Command")}
        <textarea
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          spellCheck={false}
        />
      </label>

      <button
        className="toggle-advanced"
        onClick={() => setShowAdvanced(!showAdvanced)}
      >
        {showAdvanced ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
        {t("Advanced options")}
      </button>

      {showAdvanced && (
        <div className="advanced-options">
          <label>
            {t("Stdin (piped to command)")}
            <textarea
              value={stdin}
              onChange={(e) => setStdin(e.target.value)}
              placeholder="optional input..."
              style={{ minHeight: 60 }}
            />
          </label>
          <div className="two-col">
            <label>
              {t("Timeout (seconds)")}
              <input
                type="number"
                min={1}
                max={3600}
                value={timeoutSecs}
                onChange={(e) => setTimeoutSecs(Number(e.target.value))}
              />
            </label>
            <label>
              {t("Max output (MiB)")}
              <input
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

      <label className="force-row">
        <input
          type="checkbox"
          checked={force}
          onChange={(e) => setForce(e.target.checked)}
        />
        <ShieldAlert size={14} />
        {t("Force (allow high-risk commands)")}
      </label>
      <button
        className="primary"
        onClick={handleRun}
        disabled={busy || !selectedHost}
      >
        <Play size={16} />
        {busy ? t("Running") : t("Run over SSH")}
      </button>
      <div className="terminal-output">
        {result ? (
          <>
            <div className="meta">
              exit={result.exit_code ?? "signal"} duration={result.duration_ms}ms{" "}
              <RiskBadge level={result.risk_level} />
              {result.truncated && (
                <span className="truncated-badge">{t("output truncated")}</span>
              )}
            </div>
            <pre>{result.stdout || result.stderr || t("(no output)")}</pre>
            {result.stderr && <pre className="stderr">{result.stderr}</pre>}
          </>
        ) : (
          <pre>{t("Command output will appear here.")}</pre>
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
    </section>
  );
}
