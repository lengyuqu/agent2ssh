import { Play, ShieldAlert } from "lucide-react";
import { useState } from "react";
import { api } from "../api";
import type { ExecResult, RiskLevel } from "../types";
import ApprovalDialog from "./ApprovalDialog";

type Props = {
  selectedHost: string;
  onExecComplete: () => void;
};

function RiskBadge({ level }: { level: RiskLevel }) {
  const map: Record<RiskLevel, { label: string; cls: string }> = {
    low:     { label: "low",     cls: "risk-low" },
    medium:  { label: "medium",  cls: "risk-medium" },
    high:    { label: "high",    cls: "risk-high" },
    blocked: { label: "blocked", cls: "risk-blocked" },
  };
  const { label, cls } = map[level];
  return <span className={`risk-badge ${cls}`}>{label}</span>;
}

export default function ExecPanel({ selectedHost, onExecComplete }: Props) {
  const [command, setCommand] = useState("uname -a");
  const [force, setForce] = useState(false);
  const [result, setResult] = useState<ExecResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pendingApproval, setPendingApproval] = useState(false);

  async function runCommand(withForce = false) {
    if (!selectedHost || !command.trim()) return;
    setBusy(true);
    setError(null);
    setPendingApproval(false);
    try {
      const next = await api.execSsh(selectedHost, command, withForce || force);
      setResult(next);
      onExecComplete();
    } catch (err) {
      const msg = String(err);
      // Detect high-risk rejection → show approval dialog
      if (msg.includes("force=true") || msg.includes("risk=high")) {
        setPendingApproval(true);
      } else {
        setError(msg);
      }
    } finally {
      setBusy(false);
    }
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
        Execute
      </div>
      {error && <div className="error">{error}</div>}
      <label>
        Command
        <textarea
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          spellCheck={false}
        />
      </label>
      <label className="force-row">
        <input
          type="checkbox"
          checked={force}
          onChange={(e) => setForce(e.target.checked)}
        />
        <ShieldAlert size={14} />
        Force (allow high-risk commands)
      </label>
      <button
        className="primary"
        onClick={() => runCommand()}
        disabled={busy || !selectedHost}
      >
        <Play size={16} />
        {busy ? "Running" : "Run over SSH"}
      </button>
      <div className="terminal-output">
        {result ? (
          <>
            <div className="meta">
              exit={result.exit_code ?? "signal"} duration={result.duration_ms}ms{" "}
              <RiskBadge level={result.risk_level} />
              {result.truncated && (
                <span className="truncated-badge">output truncated</span>
              )}
            </div>
            <pre>{result.stdout || result.stderr || "(no output)"}</pre>
            {result.stderr && <pre className="stderr">{result.stderr}</pre>}
          </>
        ) : (
          <pre>Command output will appear here.</pre>
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
