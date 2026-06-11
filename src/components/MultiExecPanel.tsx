import { Play, Server } from "lucide-react";
import { useState } from "react";
import { api } from "../api";
import type { ExecMultiResult, HostProfile } from "../types";
import RiskBadge from "./RiskBadge";

type Props = {
  hosts: HostProfile[];
  onExecComplete: () => void;
};

export default function MultiExecPanel({ hosts, onExecComplete }: Props) {
  const [selected, setSelected] = useState<string[]>([]);
  const [command, setCommand] = useState("");
  const [force, setForce] = useState(false);
  const [tags, setTags] = useState("");
  const [results, setResults] = useState<ExecMultiResult[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

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
    setError(null);
    try {
      const parsedTags = tags.trim() ? tags.split(",").map(t => t.trim()) : undefined;
      const res = await api.execMulti(selected, command, force, undefined, parsedTags);
      setResults(res);
      onExecComplete();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel multi-exec-panel">
      <div className="panel-title">
        <Server size={16} />
        Multi-host Execute
      </div>
      {error && <div className="error">{error}</div>}

      <div className="multi-host-picker">
        <button className="toggle-advanced" onClick={selectAll}>
          {selected.length === hosts.length ? "Deselect all" : "Select all"}
        </button>
        <div className="host-chips">
          {hosts.map((h) => (
            <button
              key={h.name}
              className={`host-chip${selected.includes(h.name) ? " active" : ""}`}
              onClick={() => toggleHost(h.name)}
            >
              {h.name}
            </button>
          ))}
          {hosts.length === 0 && <span className="empty">No hosts</span>}
        </div>
      </div>

      <label>
        Command
        <textarea
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          spellCheck={false}
          placeholder="uname -a"
        />
      </label>
      <label className="force-row">
        <input
          type="checkbox"
          checked={force}
          onChange={(e) => setForce(e.target.checked)}
        />
        Force
      </label>
      <label>
        Tags (comma-separated, optional)
        <div className="tags-input">
          <input
            value={tags}
            onChange={(e) => setTags(e.target.value)}
            placeholder="web, production"
          />
        </div>
      </label>
      <button
        className="primary"
        onClick={runMulti}
        disabled={busy || selected.length === 0 || !command.trim()}
      >
        <Play size={14} />
        {busy
          ? `Running on ${selected.length} hosts...`
          : `Run on ${selected.length} host${selected.length !== 1 ? "s" : ""}`}
      </button>

      {results.length > 0 && (
        <div className="multi-results">
          {results.map((r) => (
            <div key={r.host} className="multi-result-row">
              <strong>{r.host}</strong>
              {r.result ? (
                <>
                  <span>
                    exit={r.result.exit_code ?? "signal"} {r.result.duration_ms}ms
                  </span>
                  <RiskBadge level={r.result.risk_level} />
                </>
              ) : (
                <span className="multi-error">{r.error}</span>
              )}
            </div>
          ))}
          {results.map((r) =>
            r.result ? (
              <div key={`${r.host}-output`} className="multi-result-output">
                <div className="multi-output-header">{r.host}</div>
                <pre>{r.result.stdout || r.result.stderr || "(no output)"}</pre>
              </div>
            ) : null
          )}
        </div>
      )}
    </section>
  );
}
