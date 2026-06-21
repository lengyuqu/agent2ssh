import {
  Activity,
  CheckCircle2,
  CircleDashed,
  Clipboard,
  Download,
  ExternalLink,
  FileText,
  Lock,
  PauseCircle,
  PlayCircle,
  Power,
  RefreshCw,
  Settings,
  Square,
  Trash2,
  Upload,
  Wand2,
  XCircle,
  X,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api, getDaemonUrl, reportError } from "../api";
import { useI18n } from "../i18n";
import type { DaemonHealth, DiagnosticLogEntry, ExecutionGateStatus } from "../types";
import LanguageSwitcher from "./LanguageSwitcher";
import { THEMES, useTheme } from "../theme";
import { cn } from "../lib/utils";

type Props = {
  gateStatus: ExecutionGateStatus | null;
  gateBusy: boolean;
  gateCheckedAt: number | null;
  daemonHealth: DaemonHealth | null;
  daemonHealthCheckedAt: number | null;
  onGateToggle: () => void;
  onGateRefresh: () => void | Promise<void>;
  onDaemonHealthRefresh: () => void | Promise<void>;
  onImportConfig: () => void;
  onOpenSetup: () => void;
};

const sectionTitleCls =
  "flex items-center gap-1.5 text-xs font-bold uppercase tracking-wide text-muted-foreground";
const actionBtnCls =
  "inline-flex h-9 w-full items-center justify-center gap-2 rounded-md border border-input bg-card text-sm font-bold transition-colors hover:bg-muted disabled:pointer-events-none disabled:opacity-55";
const rowBtnCls =
  "inline-flex h-9 w-full items-center gap-2 rounded-md border border-input bg-card px-2.5 text-left text-sm font-bold transition-colors hover:bg-muted disabled:pointer-events-none disabled:opacity-55";

export default function SettingsMenu({
  gateStatus,
  gateBusy,
  gateCheckedAt,
  daemonHealth,
  daemonHealthCheckedAt,
  onGateToggle,
  onGateRefresh,
  onDaemonHealthRefresh,
  onImportConfig,
  onOpenSetup,
}: Props) {
  const { t } = useI18n();
  const { theme, setTheme } = useTheme();
  const [open, setOpen] = useState(false);
  const [refreshBusy, setRefreshBusy] = useState(false);
  const [healthBusy, setHealthBusy] = useState(false);
  const [daemonActionBusy, setDaemonActionBusy] = useState<"start" | "stop" | "restart" | null>(
    null
  );
  const [daemonActionMessage, setDaemonActionMessage] = useState<string | null>(null);
  const [copyStatus, setCopyStatus] = useState<"idle" | "copied" | "failed">("idle");
  const [diagnosticBusy, setDiagnosticBusy] = useState<"refresh" | "export" | "clear" | null>(
    null
  );
  const [diagnosticLogs, setDiagnosticLogs] = useState<DiagnosticLogEntry[]>([]);
  const [diagnosticMessage, setDiagnosticMessage] = useState<string | null>(null);
  const [appActionBusy, setAppActionBusy] = useState<"exit" | null>(null);
  // K3: in-app updater state.
  const [updateBusy, setUpdateBusy] = useState<"check" | "install" | null>(null);
  const [updateMessage, setUpdateMessage] = useState<string | null>(null);
  const [updateAvailable, setUpdateAvailable] = useState<string | null>(null);
  // K10: opt-in telemetry state.
  const [telemetryEnabled, setTelemetryEnabled] = useState(false);
  // K1: credential-store master password state.
  const [secretsInitialized, setSecretsInitialized] = useState(false);
  const [masterPassword, setMasterPassword] = useState("");
  const [secretsBusy, setSecretsBusy] = useState(false);
  const [secretsMessage, setSecretsMessage] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const gatePaused = gateStatus?.mode === "paused";
  const gateUnavailable = gateStatus === null;
  const daemonHealthy = daemonHealth?.ok === true;
  const daemonStatusText = daemonHealthy ? t("Daemon healthy") : t("Daemon unavailable");
  const daemonUrl = getDaemonUrl();
  const consoleUrl = `${daemonUrl}/console`;
  const checkedAtText = gateCheckedAt
    ? new Date(gateCheckedAt).toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      })
    : t("Never");
  const daemonCheckedAtText = daemonHealthCheckedAt
    ? new Date(daemonHealthCheckedAt).toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      })
    : t("Never");

  useEffect(() => {
    if (!open) return;

    function handlePointerDown(event: PointerEvent) {
      if (!menuRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") setOpen(false);
    }

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [open]);

  // K10: load the telemetry opt-in state when the menu opens.
  useEffect(() => {
    if (!open) return;
    void api
      .getTelemetryEnabled()
      .then(setTelemetryEnabled)
      .catch(() => setTelemetryEnabled(false));
    // K1: load credential-store status.
    void api
      .secretsStatus()
      .then((s) => setSecretsInitialized(s.initialized))
      .catch(() => setSecretsInitialized(false));
  }, [open]);

  // K1: set the master password (first time) or change it. Setting it for the
  // first time also encrypts any existing plaintext credentials.
  async function submitMasterPassword() {
    if (!masterPassword || secretsBusy) return;
    setSecretsBusy(true);
    setSecretsMessage(null);
    try {
      if (secretsInitialized) {
        await api.secretsChangePassword(masterPassword);
        setSecretsMessage(t("Master password changed"));
      } else {
        await api.secretsUnlock(masterPassword); // initializes + migrates plaintext
        setSecretsInitialized(true);
        setSecretsMessage(t("Master password set"));
      }
      setMasterPassword("");
    } catch (err) {
      setSecretsMessage(String(err));
    } finally {
      setSecretsBusy(false);
    }
  }

  async function toggleTelemetry() {
    const next = !telemetryEnabled;
    setTelemetryEnabled(next);
    try {
      await api.setTelemetryEnabled(next);
    } catch (err) {
      setTelemetryEnabled(!next); // revert on failure
      reportError("settings-menu", "toggle telemetry failed", err);
    }
  }

  async function handleGateRefresh() {
    setRefreshBusy(true);
    try {
      await onGateRefresh();
    } finally {
      setRefreshBusy(false);
    }
  }

  async function handleDaemonHealthRefresh() {
    setHealthBusy(true);
    try {
      await onDaemonHealthRefresh();
    } finally {
      setHealthBusy(false);
    }
  }

  async function runDaemonAction(action: "start" | "stop" | "restart") {
    setDaemonActionBusy(action);
    setDaemonActionMessage(null);
    try {
      const result =
        action === "start"
          ? await api.daemonStart()
          : action === "stop"
            ? await api.daemonStop()
            : await api.daemonRestart();
      setDaemonActionMessage(result.message);
      if (action !== "stop") {
        await new Promise((resolve) => window.setTimeout(resolve, 700));
      }
      await onDaemonHealthRefresh();
      await onGateRefresh();
    } catch (err) {
      setDaemonActionMessage(String(err));
      reportError("settings-menu", "daemon action failed", err, { action });
    } finally {
      setDaemonActionBusy(null);
    }
  }

  async function copyConsoleUrl() {
    setCopyStatus("idle");
    try {
      await navigator.clipboard.writeText(consoleUrl);
      setCopyStatus("copied");
      window.setTimeout(() => setCopyStatus("idle"), 1800);
    } catch {
      setCopyStatus("failed");
    }
  }

  function openConsole() {
    window.open(consoleUrl, "_blank", "noopener,noreferrer");
  }

  async function refreshDiagnostics() {
    setDiagnosticBusy("refresh");
    setDiagnosticMessage(null);
    try {
      const logs = await api.listDiagnosticLogs(20);
      setDiagnosticLogs(logs);
      setDiagnosticMessage(t("Loaded {count} diagnostic entries", { count: logs.length }));
    } catch (err) {
      setDiagnosticMessage(String(err));
    } finally {
      setDiagnosticBusy(null);
    }
  }

  // K3: check the release endpoint for a signed update.
  async function handleCheckForUpdate() {
    setUpdateBusy("check");
    setUpdateMessage(null);
    setUpdateAvailable(null);
    try {
      const { checkForUpdate } = await import("../lib/updater");
      const result = await checkForUpdate();
      if (result.status === "available") {
        setUpdateAvailable(result.version);
        setUpdateMessage(t("Update available: {version}", { version: result.version }));
      } else if (result.status === "up-to-date") {
        setUpdateMessage(t("You're on the latest version"));
      } else {
        setUpdateMessage(result.error);
      }
    } catch (err) {
      setUpdateMessage(String(err));
    } finally {
      setUpdateBusy(null);
    }
  }

  // K3: download + install the pending signed update, then relaunch.
  async function handleInstallUpdate() {
    setUpdateBusy("install");
    try {
      const { downloadAndInstallUpdate } = await import("../lib/updater");
      await downloadAndInstallUpdate((downloaded, total) => {
        setUpdateMessage(
          total
            ? t("Downloading update: {pct}%", {
                pct: Math.round((downloaded / total) * 100),
              })
            : t("Downloading update…")
        );
      });
    } catch (err) {
      setUpdateMessage(String(err));
      setUpdateBusy(null);
    }
  }

  async function exportDiagnostics() {
    setDiagnosticBusy("export");
    setDiagnosticMessage(null);
    try {
      const path = await api.exportDiagnosticBundle();
      setDiagnosticMessage(t("Diagnostic bundle written: {path}", { path }));
      await refreshDiagnostics();
    } catch (err) {
      setDiagnosticMessage(String(err));
      reportError("settings-menu", "export diagnostic bundle failed", err);
    } finally {
      setDiagnosticBusy(null);
    }
  }

  async function clearDiagnostics() {
    setDiagnosticBusy("clear");
    setDiagnosticMessage(null);
    try {
      await api.clearDiagnosticLogs();
      setDiagnosticLogs([]);
      setDiagnosticMessage(t("Diagnostic app log cleared"));
    } catch (err) {
      setDiagnosticMessage(String(err));
    } finally {
      setDiagnosticBusy(null);
    }
  }

  async function quitApplication() {
    setAppActionBusy("exit");
    try {
      await api.quitApp();
    } catch (err) {
      setDiagnosticMessage(String(err));
      reportError("settings-menu", "quit application failed", err);
    } finally {
      setAppActionBusy(null);
    }
  }

  function formatDiagnosticEntry(entry: DiagnosticLogEntry): string {
    const time = new Date(entry.ts).toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
    return `${time} ${entry.level.toUpperCase()} ${entry.component}: ${entry.message}`;
  }

  const statusCls = (variant: "active" | "paused" | "unknown") =>
    cn(
      "flex items-center gap-2 rounded-md border px-2.5 py-2 text-sm font-bold",
      variant === "active" && "border-success/40 bg-success/10 text-success",
      variant === "paused" && "border-destructive/40 bg-destructive/10 text-destructive",
      variant === "unknown" && "border-border bg-muted text-muted-foreground"
    );

  return (
    <div className="relative" ref={menuRef}>
      <button
        type="button"
        className="inline-flex h-9 items-center gap-1.5 rounded-md border border-input bg-card px-3 text-sm font-bold text-foreground/80 transition-colors hover:bg-muted hover:text-foreground"
        onClick={() => setOpen((next) => !next)}
        aria-haspopup="menu"
        aria-expanded={open}
        title={t("Settings")}
      >
        <Settings size={16} />
        <span>{t("Settings")}</span>
      </button>

      {open && (
        <div
          className="absolute right-0 top-[calc(100%+8px)] z-50 grid w-[min(360px,calc(100vw-32px))] gap-3 rounded-xl border border-border bg-popover p-3.5 text-popover-foreground shadow-2xl max-md:left-0 max-md:right-auto"
          role="menu"
        >
          <div className="flex items-start justify-between gap-3 border-b border-border pb-2.5">
            <div className="grid min-w-0 gap-0.5">
              <strong className="text-[15px]">{t("Settings")}</strong>
              <span className="text-xs leading-snug text-muted-foreground">
                {t("App preferences and safety controls")}
              </span>
            </div>
            <button
              type="button"
              className="inline-flex size-7 shrink-0 items-center justify-center rounded-md border border-border bg-card text-muted-foreground transition-colors hover:bg-muted"
              onClick={() => setOpen(false)}
              title={t("Close")}
            >
              <X size={15} />
            </button>
          </div>

          <section className="grid gap-2">
            <div className={sectionTitleCls}>
              <Activity size={15} />
              {t("Execution gate")}
            </div>
            <div className={statusCls(gateUnavailable ? "unknown" : gatePaused ? "paused" : "active")}>
              {gateUnavailable || gatePaused ? (
                <CircleDashed size={16} />
              ) : (
                <CheckCircle2 size={16} />
              )}
              <span>
                {gateUnavailable
                  ? t("Gate unavailable")
                  : gatePaused
                    ? t("Gate paused")
                    : t("Gate active")}
              </span>
            </div>
            <div className="flex flex-wrap gap-x-2.5 gap-y-1 text-xs text-muted-foreground">
              {t("Last checked: {time}", { time: checkedAtText })}
            </div>
            <button
              type="button"
              className={actionBtnCls}
              onClick={onGateToggle}
              disabled={gateBusy || gateStatus === null}
              title={gatePaused ? t("Resume execution gate") : t("Pause execution gate")}
            >
              {gatePaused ? <PlayCircle size={16} /> : <PauseCircle size={16} />}
              {gatePaused ? t("Resume") : t("Pause")}
            </button>
            <button
              type="button"
              className={actionBtnCls}
              onClick={handleGateRefresh}
              disabled={refreshBusy}
              title={t("Refresh gate status")}
            >
              <RefreshCw size={16} className={refreshBusy ? "animate-spin" : ""} />
              {t("Refresh gate status")}
            </button>
          </section>

          <section className="grid gap-2">
            <div className={sectionTitleCls}>
              <Activity size={15} />
              {t("Daemon health")}
            </div>
            <div className={statusCls(daemonHealthy ? "active" : "unknown")}>
              {daemonHealthy ? <CheckCircle2 size={16} /> : <CircleDashed size={16} />}
              <span>{daemonStatusText}</span>
            </div>
            <div className="flex flex-wrap gap-x-2.5 gap-y-1 text-xs text-muted-foreground">
              {daemonHealth?.version && (
                <span>{t("Version: {version}", { version: daemonHealth.version })}</span>
              )}
              {daemonHealth?.pid != null && <span>{t("PID: {pid}", { pid: daemonHealth.pid })}</span>}
              <span>{t("Last checked: {time}", { time: daemonCheckedAtText })}</span>
            </div>
            <button
              type="button"
              className={actionBtnCls}
              onClick={handleDaemonHealthRefresh}
              disabled={healthBusy}
              title={t("Refresh daemon health")}
            >
              <RefreshCw size={16} className={healthBusy ? "animate-spin" : ""} />
              {t("Refresh daemon health")}
            </button>
          </section>

          <section className="grid gap-2">
            <div className={sectionTitleCls}>
              <Download size={15} />
              {t("Updates")}
            </div>
            {updateMessage && (
              <div className="text-xs text-muted-foreground">{updateMessage}</div>
            )}
            <div className="grid grid-cols-2 gap-2">
              <button
                type="button"
                className={actionBtnCls}
                onClick={handleCheckForUpdate}
                disabled={updateBusy !== null}
                title={t("Check for updates")}
              >
                <RefreshCw size={16} className={updateBusy === "check" ? "animate-spin" : ""} />
                {t("Check for updates")}
              </button>
              <button
                type="button"
                className={actionBtnCls}
                onClick={handleInstallUpdate}
                disabled={updateBusy !== null || !updateAvailable}
                title={t("Install update")}
              >
                <Download size={16} className={updateBusy === "install" ? "animate-pulse" : ""} />
                {t("Install update")}
              </button>
            </div>
          </section>

          <section className="grid gap-2">
            <div className={sectionTitleCls}>
              <Lock size={15} />
              {t("Credential encryption")}
            </div>
            <p className="text-xs text-muted-foreground">
              {secretsInitialized
                ? t("Encrypted at rest with your master password (Argon2id + AES-256-GCM).")
                : t(
                    "Encrypted at rest with a master password (Argon2id + AES-256-GCM). Not yet set — credentials are stored in plaintext until you set one."
                  )}
            </p>
            {secretsMessage && (
              <div className="text-xs text-muted-foreground">{secretsMessage}</div>
            )}
            <div className="flex gap-2">
              <input
                type="password"
                value={masterPassword}
                onChange={(e) => setMasterPassword(e.target.value)}
                placeholder={secretsInitialized ? t("New master password") : t("Master password")}
                className="h-9 flex-1 rounded-md border border-input bg-background px-3 text-sm outline-none focus:ring-2 focus:ring-ring"
              />
              <button
                type="button"
                className={cn(actionBtnCls, "w-auto px-3")}
                onClick={submitMasterPassword}
                disabled={!masterPassword || secretsBusy}
              >
                {secretsInitialized ? t("Change master password") : t("Set master password")}
              </button>
            </div>
          </section>

          <section className="grid gap-2">
            <div className={sectionTitleCls}>{t("Telemetry")}</div>
            <label className="flex items-start gap-2.5 text-sm">
              <input
                type="checkbox"
                checked={telemetryEnabled}
                onChange={toggleTelemetry}
                className="mt-0.5 size-4 shrink-0 accent-primary"
              />
              <span className="text-muted-foreground">
                {t(
                  "Collect anonymous crash and usage data locally. Off by default; nothing leaves your machine until you export it."
                )}
              </span>
            </label>
          </section>

          <section className="grid gap-2">
            <div className={sectionTitleCls}>{t("Daemon controls")}</div>
            <div className="grid grid-cols-3 gap-2">
              <button
                type="button"
                className={cn(rowBtnCls, "justify-center px-2")}
                onClick={() => runDaemonAction("start")}
                disabled={daemonActionBusy !== null || daemonHealthy}
              >
                <Power size={16} />
                <span className="truncate">
                  {daemonActionBusy === "start" ? t("Starting...") : t("Start daemon")}
                </span>
              </button>
              <button
                type="button"
                className={cn(rowBtnCls, "justify-center px-2")}
                onClick={() => runDaemonAction("stop")}
                disabled={daemonActionBusy !== null || !daemonHealthy}
              >
                <Square size={16} />
                <span className="truncate">
                  {daemonActionBusy === "stop" ? t("Stopping...") : t("Stop daemon")}
                </span>
              </button>
              <button
                type="button"
                className={cn(rowBtnCls, "justify-center px-2")}
                onClick={() => runDaemonAction("restart")}
                disabled={daemonActionBusy !== null}
              >
                <RefreshCw size={16} className={daemonActionBusy === "restart" ? "animate-spin" : ""} />
                <span className="truncate">
                  {daemonActionBusy === "restart" ? t("Restarting...") : t("Restart daemon")}
                </span>
              </button>
            </div>
            {daemonActionMessage && (
              <div className="flex flex-wrap gap-x-2.5 gap-y-1 text-xs text-muted-foreground">
                {daemonActionMessage}
              </div>
            )}
          </section>

          <section className="grid gap-2">
            <div className={sectionTitleCls}>{t("Theme")}</div>
            <div className="grid grid-cols-2 gap-1.5">
              {THEMES.map((opt) => (
                <button
                  key={opt.id}
                  type="button"
                  onClick={() => setTheme(opt.id)}
                  className={cn(
                    "inline-flex items-center gap-2 rounded-md border px-2.5 py-1.5 text-sm font-medium transition-colors",
                    theme === opt.id
                      ? "border-primary bg-primary/10 text-primary"
                      : "border-input bg-card text-foreground hover:bg-muted"
                  )}
                >
                  <span
                    className="size-3.5 shrink-0 rounded-full border border-black/15"
                    style={{ background: opt.swatch }}
                  />
                  <span className="truncate">{t(opt.label)}</span>
                </button>
              ))}
            </div>
          </section>

          <section className="grid gap-2">
            <div className={sectionTitleCls}>{t("Language")}</div>
            <LanguageSwitcher />
          </section>

          <section className="grid gap-2">
            <div className={sectionTitleCls}>{t("Daemon console")}</div>
            <div
              className="truncate rounded-md border border-border bg-muted px-2.5 py-2 font-mono text-xs text-muted-foreground"
              title={consoleUrl}
            >
              {consoleUrl}
            </div>
            <button type="button" className={rowBtnCls} onClick={openConsole}>
              <ExternalLink size={16} />
              <span className="truncate">{t("Open Web Console")}</span>
            </button>
            <button type="button" className={rowBtnCls} onClick={copyConsoleUrl}>
              <Clipboard size={16} />
              <span className="truncate">
                {copyStatus === "copied"
                  ? t("Copied")
                  : copyStatus === "failed"
                    ? t("Copy failed")
                    : t("Copy console URL")}
              </span>
            </button>
          </section>

          <section className="grid gap-2">
            <div className={sectionTitleCls}>
              <FileText size={15} />
              {t("Diagnostics")}
            </div>
            <div className="grid grid-cols-3 gap-2">
              <button
                type="button"
                className={cn(rowBtnCls, "justify-center px-2")}
                onClick={refreshDiagnostics}
                disabled={diagnosticBusy !== null}
                title={t("Refresh diagnostics")}
              >
                <RefreshCw size={16} className={diagnosticBusy === "refresh" ? "animate-spin" : ""} />
                <span className="truncate">{t("Refresh")}</span>
              </button>
              <button
                type="button"
                className={cn(rowBtnCls, "justify-center px-2")}
                onClick={exportDiagnostics}
                disabled={diagnosticBusy !== null}
                title={t("Export diagnostics")}
              >
                <Download size={16} />
                <span className="truncate">{t("Export")}</span>
              </button>
              <button
                type="button"
                className={cn(rowBtnCls, "justify-center px-2")}
                onClick={clearDiagnostics}
                disabled={diagnosticBusy !== null}
                title={t("Clear app log")}
              >
                <Trash2 size={16} />
                <span className="truncate">{t("Clear")}</span>
              </button>
            </div>
            {diagnosticMessage && (
              <div className="break-words text-xs leading-snug text-muted-foreground">
                {diagnosticMessage}
              </div>
            )}
            {diagnosticLogs.length > 0 && (
              <pre className="max-h-40 overflow-auto rounded-md border border-border bg-muted p-2 font-mono text-[11px] leading-relaxed text-muted-foreground whitespace-pre-wrap break-words">
                {diagnosticLogs.map(formatDiagnosticEntry).join("\n")}
              </pre>
            )}
          </section>

          <section className="grid gap-2">
            <div className={sectionTitleCls}>{t("App")}</div>
            <button
              type="button"
              className={cn(rowBtnCls, "justify-center px-2")}
              onClick={quitApplication}
              disabled={appActionBusy !== null}
              title={t("Quit application")}
            >
              <XCircle size={16} className={appActionBusy === "exit" ? "animate-pulse" : ""} />
              <span className="truncate">
                {appActionBusy === "exit" ? t("Exiting...") : t("Quit application")}
              </span>
            </button>
          </section>

          <section className="grid gap-2">
            <div className={sectionTitleCls}>{t("Setup")}</div>
            <button
              type="button"
              className={rowBtnCls}
              onClick={() => {
                onImportConfig();
                setOpen(false);
              }}
            >
              <Upload size={16} />
              <span className="truncate">{t("Import from ~/.ssh/config")}</span>
            </button>
            <button
              type="button"
              className={rowBtnCls}
              onClick={() => {
                onOpenSetup();
                setOpen(false);
              }}
            >
              <Wand2 size={16} />
              <span className="truncate">{t("Open setup wizard")}</span>
            </button>
          </section>

          <div className="flex items-center gap-2 border-t border-border pt-3 text-xs text-muted-foreground">
            <Activity size={15} />
            {t("Local daemon embedded")}
          </div>
        </div>
      )}
    </div>
  );
}
