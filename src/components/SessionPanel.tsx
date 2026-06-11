import { Send, Terminal, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { SessionInfo } from "../types";

type Props = {
  selectedHost: string;
};

export default function SessionPanel({ selectedHost }: Props) {
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [output, setOutput] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const outputRef = useRef<HTMLPreElement>(null);

  async function refresh() {
    try {
      const list = await api.sessionList();
      setSessions(list);
    } catch (err) {
      setError(String(err));
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function openSession() {
    if (!selectedHost) return;
    setBusy(true);
    setError(null);
    try {
      const id = await api.sessionOpen(selectedHost);
      setActiveId(id);
      await refresh();
      // Initial read
      const initial = await api.sessionRead(id, 1500);
      setOutput(initial);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function sendInput() {
    if (!activeId || !input) return;
    setBusy(true);
    setError(null);
    try {
      await api.sessionWrite(activeId, input + "\n");
      setInput("");
      // Read response
      const data = await api.sessionRead(activeId, 2000);
      setOutput((prev) => prev + "\n" + data);
      setTimeout(() => {
        outputRef.current?.scrollTo(0, outputRef.current.scrollHeight);
      }, 50);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function readOutput() {
    if (!activeId) return;
    try {
      const data = await api.sessionRead(activeId, 1000);
      setOutput((prev) => prev + data);
    } catch (err) {
      setError(String(err));
    }
  }

  async function closeSession() {
    if (!activeId) return;
    try {
      await api.sessionClose(activeId);
      setActiveId(null);
      setOutput("");
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <section className="panel session-panel">
      <div className="panel-title">
        <Terminal size={16} />
        Session
        {activeId && <span className="session-active-badge">active</span>}
      </div>
      {error && <div className="error">{error}</div>}

      {!activeId ? (
        <>
          <div className="session-list">
            {sessions.map(([id, host]) => (
              <div key={id} className="session-row">
                <code>{id.slice(0, 8)}</code>
                <span>{host}</span>
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
            <button className="secondary" onClick={readOutput} disabled={busy}>
              Read
            </button>
            <button className="secondary" onClick={closeSession}>
              <X size={14} />
              Close
            </button>
          </div>
          <div className="terminal-output session-output">
            <pre ref={outputRef}>{output || "(no output yet)"}</pre>
          </div>
          <div className="session-input-row">
            <input
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  sendInput();
                }
              }}
              placeholder="Type command and press Enter..."
              disabled={busy}
            />
            <button className="primary" onClick={sendInput} disabled={busy || !input}>
              <Send size={14} />
            </button>
          </div>
        </>
      )}
    </section>
  );
}
