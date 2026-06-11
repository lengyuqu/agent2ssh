import { Play } from "lucide-react";
import { useState } from "react";
import { api } from "../api";
import type { ExecResult } from "../types";

type Props = {
  selectedHost: string;
  onExecComplete: () => void;
};

export default function ExecPanel({ selectedHost, onExecComplete }: Props) {
  const [command, setCommand] = useState("uname -a");
  const [result, setResult] = useState<ExecResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function runCommand() {
    if (!selectedHost || !command.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const next = await api.execSsh(selectedHost, command);
      setResult(next);
      onExecComplete();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
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
      <button
        className="primary"
        onClick={runCommand}
        disabled={busy || !selectedHost}
      >
        <Play size={16} />
        {busy ? "Running" : "Run over SSH"}
      </button>
      <div className="terminal-output">
        {result ? (
          <>
            <div className="meta">
              exit={result.exit_code ?? "signal"} duration={result.duration_ms}ms
            </div>
            <pre>{result.stdout || result.stderr || "(no output)"}</pre>
            {result.stderr && <pre className="stderr">{result.stderr}</pre>}
          </>
        ) : (
          <pre>Command output will appear here.</pre>
        )}
      </div>
    </section>
  );
}
