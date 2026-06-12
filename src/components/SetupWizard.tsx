import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { HostProfile, SshKeyInfo } from "../types";

interface SetupWizardProps {
  onComplete: () => void;
  onSkip: () => void;
}

const TOTAL_STEPS = 5;

const STEP_TITLES = [
  "Welcome to Agent2SSH",
  "Import SSH Config",
  "SSH Key",
  "Start Daemon",
  "Open Web Console",
];

export default function SetupWizard({ onComplete, onSkip }: SetupWizardProps) {
  const [step, setStep] = useState(0);
  const [importedHosts, setImportedHosts] = useState<HostProfile[]>([]);
  const [importError, setImportError] = useState<string | null>(null);
  const [importing, setImporting] = useState(false);
  const [keys, setKeys] = useState<SshKeyInfo[]>([]);
  const [keyName, setKeyName] = useState("");
  const [generatingKey, setGeneratingKey] = useState(false);
  const [keyError, setKeyError] = useState<string | null>(null);
  const [daemonRunning, setDaemonRunning] = useState(false);
  const [daemonLoading, setDaemonLoading] = useState(false);
  const [daemonError, setDaemonError] = useState<string | null>(null);

  const loadKeys = useCallback(async () => {
    try {
      const k = await api.listKeys();
      setKeys(k);
    } catch {
      // silent
    }
  }, []);

  const checkDaemon = useCallback(async () => {
    try {
      const res = await fetch("http://127.0.0.1:7722/health");
      setDaemonRunning(res.ok);
    } catch {
      setDaemonRunning(false);
    }
  }, []);

  useEffect(() => {
    if (step === 2) loadKeys();
    if (step === 3) checkDaemon();
  }, [step, loadKeys, checkDaemon]);

  async function handleImport() {
    setImporting(true);
    setImportError(null);
    try {
      const hosts = await api.importSshConfig();
      setImportedHosts(hosts);
    } catch (err) {
      setImportError(String(err));
    } finally {
      setImporting(false);
    }
  }

  async function handleGenerateKey() {
    if (!keyName.trim()) {
      setKeyError("Please enter a key name.");
      return;
    }
    setGeneratingKey(true);
    setKeyError(null);
    try {
      const key = await api.generateKey(keyName.trim());
      setKeys((prev) => [...prev, key]);
      setKeyName("");
    } catch (err) {
      setKeyError(String(err));
    } finally {
      setGeneratingKey(false);
    }
  }

  async function handleStartDaemon() {
    setDaemonLoading(true);
    setDaemonError(null);
    try {
      // The Tauri app embeds the daemon internally; we simply check if it's
      // already reachable. If not, we inform the user to start it via CLI.
      await checkDaemon();
      if (!daemonRunning) {
        setDaemonError(
          "The embedded daemon is not reachable. Start it via: agent2ssh daemon start"
        );
      }
    } catch (err) {
      setDaemonError(String(err));
    } finally {
      setDaemonLoading(false);
    }
  }

  function handleOpenConsole() {
    window.open("http://127.0.0.1:7722/console", "_blank");
    onComplete();
  }

  function next() {
    if (step < TOTAL_STEPS - 1) setStep(step + 1);
    else onComplete();
  }

  function prev() {
    if (step > 0) setStep(step - 1);
  }

  return (
    <div className="wizard-overlay">
      <div className="wizard-card">
        {/* Step indicator */}
        <div className="wizard-header">
          <span className="wizard-step-badge">
            {step + 1}/{TOTAL_STEPS}
          </span>
          <h2>{STEP_TITLES[step]}</h2>
          <button className="wizard-skip-btn" onClick={onSkip} title="Skip setup">
            Skip setup
          </button>
        </div>

        {/* Step content */}
        <div className="wizard-body">
          {step === 0 && (
            <div className="wizard-step">
              <p>
                Agent2SSH is a local SSH capability layer that lets AI agents
                and automation tools securely execute commands on remote hosts.
              </p>
              <ul className="wizard-feature-list">
                <li>Import hosts from your existing SSH config</li>
                <li>Risk-based command classification and approval flow</li>
                <li>Persistent sessions, port forwarding, and SFTP</li>
                <li>MCP server for AI agent integration</li>
              </ul>
              <p>This wizard will help you get started in a few steps.</p>
            </div>
          )}

          {step === 1 && (
            <div className="wizard-step">
              <p>
                Import host profiles from <code>~/.ssh/config</code>. Existing
                profiles will not be overwritten.
              </p>
              <button
                className="primary"
                onClick={handleImport}
                disabled={importing}
              >
                {importing ? "Importing..." : "Import from ~/.ssh/config"}
              </button>
              {importError && <div className="error">{importError}</div>}
              {importedHosts.length > 0 && (
                <div className="wizard-preview">
                  <p>
                    Imported {importedHosts.length} host(s):
                  </p>
                  <ul>
                    {importedHosts.map((h) => (
                      <li key={h.name}>
                        <strong>{h.name}</strong> &rarr;{" "}
                        {h.user ? `${h.user}@` : ""}
                        {h.host}:{h.port ?? 22}
                      </li>
                    ))}
                  </ul>
                </div>
              )}
              {!importing && !importError && importedHosts.length === 0 && (
                <p className="wizard-hint">
                  No new hosts found. You may not have a{" "}
                  <code>~/.ssh/config</code> file, or all hosts are already
                  imported. You can add hosts manually later.
                </p>
              )}
            </div>
          )}

          {step === 2 && (
            <div className="wizard-step">
              <p>
                SSH keys are used for passwordless authentication to remote
                hosts.
              </p>
              {keys.length > 0 ? (
                <div className="wizard-preview">
                  <p>Existing keys:</p>
                  <ul>
                    {keys.map((k) => (
                      <li key={k.name}>
                        <strong>{k.name}</strong> ({k.key_type})
                      </li>
                    ))}
                  </ul>
                </div>
              ) : (
                <p className="wizard-hint">No SSH keys found in Agent2SSH.</p>
              )}
              <div className="wizard-inline-form">
                <input
                  type="text"
                  placeholder="New key name (e.g. id_ed25519)"
                  value={keyName}
                  onChange={(e) => setKeyName(e.target.value)}
                />
                <button
                  className="secondary"
                  onClick={handleGenerateKey}
                  disabled={generatingKey}
                >
                  {generatingKey ? "Generating..." : "Generate Key"}
                </button>
              </div>
              {keyError && <div className="error">{keyError}</div>}
            </div>
          )}

          {step === 3 && (
            <div className="wizard-step">
              <p>
                The daemon provides the HTTP API and web console. When running,
                it listens on <code>127.0.0.1:7722</code>.
              </p>
              <div className="wizard-daemon-status">
                {daemonRunning ? (
                  <div className="wizard-status-ok">
                    Daemon is running on http://127.0.0.1:7722
                  </div>
                ) : (
                  <div className="wizard-status-warn">
                    Daemon is not running.
                  </div>
                )}
              </div>
              <button
                className="primary"
                onClick={handleStartDaemon}
                disabled={daemonLoading}
              >
                {daemonLoading ? "Checking..." : "Check Daemon Status"}
              </button>
              {daemonError && <div className="error">{daemonError}</div>}
              <p className="wizard-hint">
                You can also start the daemon from a terminal:{" "}
                <code>agent2ssh daemon start</code>
              </p>
            </div>
          )}

          {step === 4 && (
            <div className="wizard-step">
              <p>
                The web console provides a browser-based interface for managing
                hosts, executing commands, and viewing audit logs.
              </p>
              <div className="wizard-console-link">
                <code>http://127.0.0.1:7722/console</code>
              </div>
              <button className="primary" onClick={handleOpenConsole}>
                Open Web Console
              </button>
              <p className="wizard-hint">
                You can also access this from the daemon URL at any time.
              </p>
            </div>
          )}
        </div>

        {/* Navigation */}
        <div className="wizard-footer">
          <button
            className="secondary"
            onClick={prev}
            disabled={step === 0}
          >
            Back
          </button>
          <div className="wizard-dots">
            {Array.from({ length: TOTAL_STEPS }).map((_, i) => (
              <span
                key={i}
                className={`wizard-dot ${i === step ? "active" : ""}`}
              />
            ))}
          </div>
          {step < TOTAL_STEPS - 1 ? (
            <button className="primary" onClick={next}>
              Next
            </button>
          ) : (
            <button className="primary" onClick={onComplete}>
              Finish
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
