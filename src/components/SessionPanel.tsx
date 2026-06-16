import { Eye, PlugZap, RefreshCw, Send, Terminal, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { RiskLevel } from "../types";

type Props = {
  selectedHost: string;
};

type SessionBackend = "daemon" | "local";

type ManagedSession = {
  id: string;
  host: string;
  backend: SessionBackend;
};

export default function SessionPanel({ selectedHost }: Props) {
  const [sessions, setSessions] = useState<ManagedSession[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [activeHost, setActiveHost] = useState<string | null>(null);
  const [activeBackend, setActiveBackend] = useState<SessionBackend>("daemon");
  const [input, setInput] = useState("");
  const [output, setOutput] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [registryMode, setRegistryMode] = useState<SessionBackend>("daemon");
  const [autoTail, setAutoTail] = useState(false);
  const [readOnly, setReadOnly] = useState(false);
  const [pendingDanger, setPendingDanger] = useState<{
    input: string;
    risk: RiskLevel;
  } | null>(null);
  const outputRef = useRef<HTMLPreElement>(null);
  const readingRef = useRef(false);

  async function refresh() {
    try {
      const list = await api.sessionListDaemon();
      setSessions(list.map((session) => ({ ...session, backend: "daemon" })));
      setRegistryMode("daemon");
    } catch (err) {
      try {
        const list = await api.sessionList();
        setSessions(list.map(([id, host]) => ({ id, host, backend: "local" })));
        setRegistryMode("local");
      } catch (fallbackErr) {
        setError(`${String(err)}; ${String(fallbackErr)}`);
      }
    }
  }

  useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, 3000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!autoTail || !activeId) return;
    const timer = window.setInterval(() => {
      if (!busy) void readOutput();
    }, 2000);
    return () => window.clearInterval(timer);
  }, [activeId, activeBackend, autoTail, busy]);

  // Reset when selectedHost changes
  useEffect(() => {
    setError(null);
  }, [selectedHost]);

  async function openSession() {
    if (!selectedHost) return;
    setBusy(true);
    setError(null);
    try {
      let id: string;
      let backend: SessionBackend = "daemon";
      try {
        id = await api.sessionOpenDaemon(selectedHost);
      } catch {
        id = await api.sessionOpen(selectedHost);
        backend = "local";
      }
      setActiveId(id);
      setActiveHost(selectedHost);
      setActiveBackend(backend);
      setReadOnly(false);
      setPendingDanger(null);
      await refresh();
      const initial = await readFromBackend(id, backend, 1500);
      setOutput(initial);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function attachSession(session: ManagedSession, readOnlyMode: boolean) {
    setBusy(true);
    setError(null);
    try {
      setActiveId(session.id);
      setActiveHost(session.host);
      setActiveBackend(session.backend);
      setReadOnly(readOnlyMode);
      setPendingDanger(null);
      const initial = await readFromBackend(session.id, session.backend, 500);
      setOutput(initial);
      scrollOutput();
    } catch (err) {
      setActiveId(null);
      setActiveHost(null);
      setOutput("");
      setError(String(err));
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function readFromBackend(id: string, backend: SessionBackend, timeoutMs: number) {
    return backend === "daemon"
      ? api.sessionReadDaemon(id, timeoutMs)
      : api.sessionRead(id, timeoutMs);
  }

  async function writeToBackend(id: string, backend: SessionBackend, value: string) {
    return backend === "daemon"
      ? api.sessionWriteDaemon(id, value)
      : api.sessionWrite(id, value);
  }

  async function closeFromBackend(id: string, backend: SessionBackend) {
    return backend === "daemon"
      ? api.sessionCloseDaemon(id)
      : api.sessionClose(id);
  }

  function scrollOutput() {
    setTimeout(() => {
      outputRef.current?.scrollTo(0, outputRef.current.scrollHeight);
    }, 50);
  }

  async function sendInput(confirmDanger = false) {
    if (!activeId || !input) return;
    if (readOnly) {
      setError("Session is attached in read-only mode.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (!confirmDanger) {
        const risk = await api.classifyRisk(input);
        if (risk === "high" || risk === "blocked") {
          setPendingDanger({ input, risk });
          return;
        }
      }
      const value = confirmDanger && pendingDanger ? pendingDanger.input : input;
      await writeToBackend(activeId, activeBackend, value + "\n");
      setInput("");
      setPendingDanger(null);
      const data = await readFromBackend(activeId, activeBackend, 2000);
      setOutput((prev) => prev + "\n" + data);
      scrollOutput();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function readOutput() {
    if (!activeId) return;
    if (readingRef.current) return;
    readingRef.current = true;
    try {
      const data = await readFromBackend(activeId, activeBackend, 1000);
      if (data) {
        setOutput((prev) => prev + data);
        scrollOutput();
      }
    } catch (err) {
      setError(String(err));
      await refresh();
    } finally {
      readingRef.current = false;
    }
  }

  async function closeSession() {
    if (!activeId) return;
    try {
      await closeFromBackend(activeId, activeBackend);
      setActiveId(null);
      setActiveHost(null);
      setOutput("");
      setReadOnly(false);
      setPendingDanger(null);
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  const hostMismatch = activeId && activeHost && activeHost !== selectedHost;

  return (
    <section className="panel session-panel">
      <div className="panel-title">
        <Terminal size={16} />
        Session
        <span className="session-registry-badge">{registryMode}</span>
        {activeId && <span className="session-active-badge">active</span>}
      </div>
      {error && <div className="error">{error}</div>}

      {hostMismatch && (
        <div className="session-warning">
          Session is connected to <strong>{activeHost}</strong>, not the
          currently selected host.
        </div>
      )}

      {!activeId ? (
        <>
          <div className="session-list">
            {sessions.map((session) => (
              <div key={session.id} className="session-row">
                <code title={session.id}>{session.id.slice(0, 8)}</code>
                <span>{session.host}</span>
                <span className="session-source">{session.backend}</span>
                <button
                  className="secondary session-attach"
                  onClick={() => attachSession(session, false)}
                  disabled={busy}
                  title="Attach to this session"
                  aria-label={`Attach to ${session.host} session ${session.id.slice(0, 8)}`}
                >
                  <PlugZap size={14} />
                </button>
                <button
                  className="secondary session-attach"
                  onClick={() => attachSession(session, true)}
                  disabled={busy}
                  title="Attach read-only"
                  aria-label={`Attach read-only to ${session.host} session ${session.id.slice(0, 8)}`}
                >
                  <Eye size={14} />
                </button>
              </div>
            ))}
            {sessions.length === 0 && (
              <div className="empty">No active sessions</div>
            )}
          </div>
          <button
            className="primary"
            onClick={openSession}
            disabled={busy || !selectedHost}
          >
            <Terminal size={14} />
            {busy ? "Opening..." : "Open session"}
          </button>
        </>
      ) : (
        <>
          <div className="session-controls">
            <button
              className="secondary"
              onClick={readOutput}
              disabled={busy}
              title="Read session output"
              aria-label="Read session output"
            >
              <RefreshCw size={14} />
            </button>
            <button className="secondary" onClick={closeSession}>
              <X size={14} />
              Close
            </button>
            <label className="session-toggle">
              <input
                type="checkbox"
                checked={autoTail}
                onChange={(e) => setAutoTail(e.target.checked)}
              />
              Tail
            </label>
            <label className="session-toggle">
              <input
                type="checkbox"
                checked={readOnly}
                onChange={(e) => {
                  setReadOnly(e.target.checked);
                  setPendingDanger(null);
                }}
              />
              Read-only
            </label>
            <span className="session-active-meta">
              {activeHost} / {activeBackend} / {activeId.slice(0, 8)}
            </span>
          </div>
          {pendingDanger && (
            <div className="session-danger-confirm">
              <span>
                {pendingDanger.risk} input: <code>{pendingDanger.input}</code>
              </span>
              <button
                className="secondary"
                onClick={() => {
                  setPendingDanger(null);
                }}
              >
                Cancel
              </button>
              <button
                className="primary"
                onClick={() => sendInput(true)}
                disabled={busy}
              >
                Send anyway
              </button>
            </div>
          )}
          <div className="terminal-output session-output">
            <pre ref={outputRef}>{output || "(no output yet)"}</pre>
          </div>
          <div className="session-input-row">
            <input
              value={input}
              onChange={(e) => {
                setInput(e.target.value);
                setPendingDanger(null);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  sendInput();
                }
              }}
              placeholder="Type command and press Enter..."
              disabled={busy || readOnly}
            />
            <button
              className="primary"
              onClick={() => sendInput()}
              disabled={busy || !input || readOnly}
              title={readOnly ? "Read-only session" : "Send input"}
              aria-label={readOnly ? "Read-only session" : "Send input"}
            >
              <Send size={14} />
            </button>
          </div>
        </>
      )}
    </section>
  );
}
