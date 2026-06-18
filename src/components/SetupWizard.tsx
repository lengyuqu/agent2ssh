import { useCallback, useEffect, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { HostProfile, SshKeyInfo } from "../types";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { cn } from "../lib/utils";

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

const codeCls = "rounded bg-muted px-1.5 py-0.5 font-mono text-sm text-primary";
const hintCls = "text-sm text-muted-foreground";
const stepTextCls = "m-0 leading-relaxed text-foreground/80";
const previewCls = "rounded-lg border border-border bg-muted/40 p-3.5";
const errorCls =
  "rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive";

export default function SetupWizard({ onComplete, onSkip }: SetupWizardProps) {
  const { t } = useI18n();
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
    const health = await api.getDaemonHealth();
    const running = health?.ok === true;
    setDaemonRunning(running);
    return running;
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
      reportError("setup-wizard", "import ssh config failed", err);
    } finally {
      setImporting(false);
    }
  }

  async function handleGenerateKey() {
    if (!keyName.trim()) {
      setKeyError(t("Please enter a key name."));
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
      reportError("setup-wizard", "generate key failed", err);
    } finally {
      setGeneratingKey(false);
    }
  }

  async function handleStartDaemon() {
    setDaemonLoading(true);
    setDaemonError(null);
    try {
      if (!daemonRunning) {
        await api.daemonStart();
        await new Promise((resolve) => window.setTimeout(resolve, 700));
      }
      const running = await checkDaemon();
      if (!running) {
        setDaemonError(t("Daemon started, but the health endpoint is not reachable yet."));
      }
    } catch (err) {
      setDaemonError(String(err));
      reportError("setup-wizard", "start daemon failed", err);
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
    <div className="flex min-h-screen items-center justify-center bg-background p-6">
      <div className="w-full max-w-[560px] overflow-hidden rounded-xl border border-border bg-card shadow-xl">
        {/* Step indicator */}
        <div className="flex items-center gap-3 border-b border-border px-6 py-4">
          <span className="inline-flex h-7 min-w-9 items-center justify-center rounded-full bg-primary px-2 text-xs font-semibold text-primary-foreground">
            {step + 1}/{TOTAL_STEPS}
          </span>
          <h2 className="m-0 flex-1 text-lg font-semibold">{t(STEP_TITLES[step])}</h2>
          <button
            className="rounded px-2 py-1 text-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            onClick={onSkip}
            title={t("Skip setup")}
          >
            {t("Skip setup")}
          </button>
        </div>

        {/* Step content */}
        <div className="min-h-[200px] p-6">
          {step === 0 && (
            <div className="flex flex-col gap-3.5">
              <p className={stepTextCls}>
                {t("Agent2SSH is a local SSH capability layer that lets AI agents and automation tools securely execute commands on remote hosts.")}
              </p>
              <ul className="m-0 list-disc pl-5">
                <li className="leading-relaxed text-foreground/80">
                  {t("Import hosts from your existing SSH config")}
                </li>
                <li className="leading-relaxed text-foreground/80">
                  {t("Risk-based command classification and approval flow")}
                </li>
                <li className="leading-relaxed text-foreground/80">
                  {t("Persistent sessions, port forwarding, and SFTP")}
                </li>
                <li className="leading-relaxed text-foreground/80">
                  {t("MCP server for AI agent integration")}
                </li>
              </ul>
              <p className={stepTextCls}>{t("This wizard will help you get started in a few steps.")}</p>
            </div>
          )}

          {step === 1 && (
            <div className="flex flex-col gap-3.5">
              <p className={stepTextCls}>
                {t("Import host profiles from ~/.ssh/config. Existing profiles will not be overwritten.")}
              </p>
              <Button onClick={handleImport} disabled={importing} className="self-start">
                {importing ? t("Importing...") : t("Import from ~/.ssh/config")}
              </Button>
              {importError && <div className={errorCls}>{importError}</div>}
              {importedHosts.length > 0 && (
                <div className={previewCls}>
                  <p className="m-0 mb-2 text-sm font-semibold">
                    {t("Imported {count} host(s):", { count: importedHosts.length })}
                  </p>
                  <ul className="m-0 list-disc pl-[18px]">
                    {importedHosts.map((h) => (
                      <li key={h.name} className="text-sm leading-relaxed">
                        <strong>{h.name}</strong> &rarr;{" "}
                        {h.user ? `${h.user}@` : ""}
                        {h.host}:{h.port ?? 22}
                      </li>
                    ))}
                  </ul>
                </div>
              )}
              {!importing && !importError && importedHosts.length === 0 && (
                <p className={hintCls}>
                  {t("No new hosts found. You may not have a ~/.ssh/config file, or all hosts are already imported. You can add hosts manually later.")}
                </p>
              )}
            </div>
          )}

          {step === 2 && (
            <div className="flex flex-col gap-3.5">
              <p className={stepTextCls}>
                {t("SSH keys are used for passwordless authentication to remote hosts.")}
              </p>
              {keys.length > 0 ? (
                <div className={previewCls}>
                  <p className="m-0 mb-2 text-sm font-semibold">{t("Existing keys:")}</p>
                  <ul className="m-0 list-disc pl-[18px]">
                    {keys.map((k) => (
                      <li key={k.name} className="text-sm leading-relaxed">
                        <strong>{k.name}</strong> ({k.key_type})
                      </li>
                    ))}
                  </ul>
                </div>
              ) : (
                <p className={hintCls}>{t("No SSH keys found in Agent2SSH.")}</p>
              )}
              <div className="flex gap-2">
                <Input
                  type="text"
                  placeholder={t("New key name (e.g. id_ed25519)")}
                  value={keyName}
                  onChange={(e) => setKeyName(e.target.value)}
                />
                <Button
                  variant="secondary"
                  onClick={handleGenerateKey}
                  disabled={generatingKey}
                  className="shrink-0"
                >
                  {generatingKey ? t("Generating...") : t("Generate Key")}
                </Button>
              </div>
              {keyError && <div className={errorCls}>{keyError}</div>}
            </div>
          )}

          {step === 3 && (
            <div className="flex flex-col gap-3.5">
              <p className={stepTextCls}>
                {t("The daemon provides the HTTP API and web console. When running, it listens on 127.0.0.1:7722.")}
              </p>
              <div className="py-1">
                {daemonRunning ? (
                  <div className="rounded-md bg-success/12 px-3.5 py-2.5 text-sm font-medium text-success">
                    {t("Daemon is running on http://127.0.0.1:7722")}
                  </div>
                ) : (
                  <div className="rounded-md bg-warning/15 px-3.5 py-2.5 text-sm font-medium text-warning">
                    {t("Daemon is not running.")}
                  </div>
                )}
              </div>
              <Button onClick={handleStartDaemon} disabled={daemonLoading} className="self-start">
                {daemonLoading
                  ? t("Checking...")
                  : daemonRunning
                    ? t("Check Daemon Status")
                    : t("Start Daemon")}
              </Button>
              {daemonError && <div className={errorCls}>{daemonError}</div>}
              <p className={hintCls}>
                {t("You can also manage the daemon from Settings or a terminal:")}{" "}
                <code className={codeCls}>agent2ssh daemon start</code>
              </p>
            </div>
          )}

          {step === 4 && (
            <div className="flex flex-col gap-3.5">
              <p className={stepTextCls}>
                {t("The web console provides a browser-based interface for managing hosts, executing commands, and viewing audit logs.")}
              </p>
              <div className="rounded-lg border border-border bg-muted/40 px-4 py-3 text-center">
                <code className="font-mono text-[15px] text-primary">
                  http://127.0.0.1:7722/console
                </code>
              </div>
              <Button onClick={handleOpenConsole} className="self-start">
                {t("Open Web Console")}
              </Button>
              <p className={hintCls}>
                {t("You can also access this from the daemon URL at any time.")}
              </p>
            </div>
          )}
        </div>

        {/* Navigation */}
        <div className="flex items-center justify-between border-t border-border bg-muted/30 px-6 py-4">
          <Button variant="secondary" onClick={prev} disabled={step === 0}>
            {t("Back")}
          </Button>
          <div className="flex gap-1.5">
            {Array.from({ length: TOTAL_STEPS }).map((_, i) => (
              <span
                key={i}
                className={cn("size-2 rounded-full transition-colors", i === step ? "bg-primary" : "bg-border")}
              />
            ))}
          </div>
          {step < TOTAL_STEPS - 1 ? (
            <Button onClick={next}>{t("Next")}</Button>
          ) : (
            <Button onClick={onComplete}>{t("Finish")}</Button>
          )}
        </div>
      </div>
    </div>
  );
}
