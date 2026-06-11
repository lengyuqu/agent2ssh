import { BookOpen, Play, CheckCircle, XCircle, Loader2 } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import type { HostProfile, Playbook, PlaybookRunResult } from "../types";

type Props = {
  hosts: HostProfile[];
};

export default function PlaybooksPanel({ hosts }: Props) {
  const [playbooks, setPlaybooks] = useState<Playbook[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [selectedPlaybook, setSelectedPlaybook] = useState<string | null>(null);
  const [selectedHost, setSelectedHost] = useState("");
  const [force, setForce] = useState(false);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<PlaybookRunResult | null>(null);

  async function refresh() {
    try {
      const list = await api.listPlaybooks();
      setPlaybooks(list);
    } catch {
      // playbooks file might not exist yet
      setPlaybooks([]);
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  function showRunForm(name: string) {
    setSelectedPlaybook(name);
    setResult(null);
    if (hosts.length > 0 && !selectedHost) {
      setSelectedHost(hosts[0].name);
    }
  }

  function hideRunForm() {
    setSelectedPlaybook(null);
    setResult(null);
  }

  async function handleRun() {
    if (!selectedPlaybook || !selectedHost) {
      setError("Select a playbook and target host");
      return;
    }
    setError(null);
    setRunning(true);
    setResult(null);
    try {
      const res = await api.runPlaybook(selectedPlaybook, selectedHost, force);
      setResult(res);
    } catch (err) {
      setError(String(err));
    } finally {
      setRunning(false);
    }
  }

  return (
    <div className="panel">
      <div className="panel-title">
        <BookOpen size={16} />
        Playbooks
        <button className="icon-button" title="Refresh" onClick={refresh}>
          &#8635;
        </button>
      </div>

      {error && <div className="error">{error}</div>}

      {playbooks.length === 0 && (
        <p className="empty">
          No playbooks configured. Add playbooks to{" "}
          <code>~/.agent2ssh/playbooks.toml</code>.
        </p>
      )}

      {playbooks.length > 0 && (
        <div className="playbook-list">
          {playbooks.map((pb) => (
            <div
              key={pb.name}
              className="playbook-row"
              style={{
                display: "flex",
                alignItems: "center",
                gap: 12,
                padding: "8px 10px",
                background: "#f8fafc",
                border: "1px solid #e2e8f0",
                borderRadius: 6,
                marginBottom: 6,
              }}
            >
              <div style={{ flex: 1 }}>
                <div style={{ fontWeight: 600 }}>{pb.name}</div>
                <div
                  style={{
                    color: "#64748b",
                    fontSize: 13,
                    marginTop: 2,
                  }}
                >
                  {pb.description}
                </div>
                <div
                  style={{
                    color: "#94a3b8",
                    fontSize: 12,
                    marginTop: 2,
                    display: "flex",
                    gap: 8,
                    flexWrap: "wrap",
                  }}
                >
                  <span>{pb.steps.length} step{pb.steps.length !== 1 ? "s" : ""}</span>
                  {pb.tags.map((t) => (
                    <span key={t} className="tag-badge">
                      {t}
                    </span>
                  ))}
                  {pb.risk_override && (
                    <span className={`risk-badge risk-${pb.risk_override}`}>
                      {pb.risk_override}
                    </span>
                  )}
                </div>
              </div>
              <button
                className="primary"
                style={{ fontSize: 13, minHeight: 32, padding: "4px 14px" }}
                onClick={() => showRunForm(pb.name)}
              >
                <Play size={13} />
                Run
              </button>
            </div>
          ))}
        </div>
      )}

      {selectedPlaybook && (
        <div
          className="playbook-run-form"
          style={{
            marginTop: 12,
            padding: 12,
            background: "#f1f5f9",
            border: "1px solid #cbd5e1",
            borderRadius: 6,
          }}
        >
          <div style={{ fontWeight: 600, marginBottom: 8 }}>
            Run: <span className="mono">{selectedPlaybook}</span>
          </div>
          <div style={{ display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
            <select
              value={selectedHost}
              onChange={(e) => setSelectedHost(e.target.value)}
              style={{ minWidth: 140 }}
            >
              {hosts.length === 0 && <option value="">No hosts</option>}
              {hosts.map((h) => (
                <option key={h.name} value={h.name}>
                  {h.name}
                </option>
              ))}
            </select>
            <label
              style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                cursor: "pointer",
                fontWeight: 600,
                color: force ? "#9f1239" : "#41515c",
              }}
            >
              <input
                type="checkbox"
                checked={force}
                onChange={(e) => setForce(e.target.checked)}
                style={{ width: "auto" }}
              />
              Force
            </label>
            <button
              className="primary"
              disabled={running || !selectedHost}
              onClick={handleRun}
            >
              {running ? (
                <>
                  <Loader2 size={14} className="spin" /> Running...
                </>
              ) : (
                <>
                  <Play size={14} /> Execute
                </>
              )}
            </button>
            <button className="secondary" onClick={hideRunForm}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {result && (
        <div style={{ marginTop: 12 }}>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              marginBottom: 8,
              fontWeight: 600,
            }}
          >
            {result.success ? (
              <span style={{ color: "#16a34a" }}>
                <CheckCircle size={16} /> Success
              </span>
            ) : (
              <span style={{ color: "#dc2626" }}>
                <XCircle size={16} /> Failed
              </span>
            )}
            <span style={{ color: "#64748b", fontSize: 13, marginLeft: "auto" }}>
              {result.steps_completed.length}/{playbooks.find((p) => p.name === result.playbook)?.steps.length ?? "?"} steps
              &nbsp;&middot;&nbsp;
              {result.total_duration_ms < 1000
                ? `${result.total_duration_ms}ms`
                : `${(result.total_duration_ms / 1000).toFixed(2)}s`}
            </span>
          </div>

          <div className="playbook-results">
            {result.steps_completed.map((s) => {
              const ok = s.result && s.result.exit_code === 0;
              return (
                <div
                  key={s.step}
                  className={`step-result ${ok ? "step-success" : "step-failure"}`}
                  style={{
                    padding: "8px 10px",
                    border: `1px solid ${ok ? "#bbf7d0" : "#fecaca"}`,
                    borderRadius: 6,
                    marginBottom: 6,
                    background: ok ? "#f0fdf4" : "#fef2f2",
                  }}
                >
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                    }}
                  >
                    {ok ? (
                      <CheckCircle size={14} style={{ color: "#16a34a" }} />
                    ) : (
                      <XCircle size={14} style={{ color: "#dc2626" }} />
                    )}
                    <span className="mono" style={{ flex: 1 }}>
                      {s.step + 1}. {s.command}
                    </span>
                    {s.result && (
                      <span style={{ color: "#64748b", fontSize: 12 }}>
                        exit={s.result.exit_code ?? "n/a"}{" "}
                        {s.result.duration_ms < 1000
                          ? `${s.result.duration_ms}ms`
                          : `${(s.result.duration_ms / 1000).toFixed(2)}s`}
                      </span>
                    )}
                  </div>
                  {s.result?.stdout && (
                    <pre
                      style={{
                        margin: "4px 0 0",
                        fontSize: 12,
                        maxHeight: 120,
                        overflow: "auto",
                        whiteSpace: "pre-wrap",
                        wordBreak: "break-word",
                      }}
                    >
                      {s.result.stdout}
                    </pre>
                  )}
                  {s.result?.stderr && (
                    <pre
                      style={{
                        margin: "4px 0 0",
                        fontSize: 12,
                        color: "#dc2626",
                        maxHeight: 80,
                        overflow: "auto",
                        whiteSpace: "pre-wrap",
                        wordBreak: "break-word",
                      }}
                    >
                      {s.result.stderr}
                    </pre>
                  )}
                  {s.error && (
                    <div
                      style={{
                        marginTop: 4,
                        color: "#dc2626",
                        fontSize: 12,
                      }}
                    >
                      {s.error}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
