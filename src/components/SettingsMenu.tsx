import {
  Activity,
  CheckCircle2,
  CircleDashed,
  Clipboard,
  ExternalLink,
  PauseCircle,
  PlayCircle,
  RefreshCw,
  Settings,
  Upload,
  Wand2,
  X,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { getDaemonUrl } from "../api";
import { useI18n } from "../i18n";
import type { ExecutionGateStatus } from "../types";
import LanguageSwitcher from "./LanguageSwitcher";

type Props = {
  gateStatus: ExecutionGateStatus | null;
  gateBusy: boolean;
  gateCheckedAt: number | null;
  onGateToggle: () => void;
  onGateRefresh: () => void | Promise<void>;
  onImportConfig: () => void;
  onOpenSetup: () => void;
};

export default function SettingsMenu({
  gateStatus,
  gateBusy,
  gateCheckedAt,
  onGateToggle,
  onGateRefresh,
  onImportConfig,
  onOpenSetup,
}: Props) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [refreshBusy, setRefreshBusy] = useState(false);
  const [copyStatus, setCopyStatus] = useState<"idle" | "copied" | "failed">("idle");
  const menuRef = useRef<HTMLDivElement>(null);
  const gatePaused = gateStatus?.mode === "paused";
  const gateUnavailable = gateStatus === null;
  const daemonUrl = getDaemonUrl();
  const consoleUrl = `${daemonUrl}/console`;
  const checkedAtText = gateCheckedAt
    ? new Date(gateCheckedAt).toLocaleTimeString([], {
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

  async function handleGateRefresh() {
    setRefreshBusy(true);
    try {
      await onGateRefresh();
    } finally {
      setRefreshBusy(false);
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

  return (
    <div className="settings-menu" ref={menuRef}>
      <button
        type="button"
        className="settings-trigger"
        onClick={() => setOpen((next) => !next)}
        aria-haspopup="menu"
        aria-expanded={open}
        title={t("Settings")}
      >
        <Settings size={16} />
        <span>{t("Settings")}</span>
      </button>

      {open && (
        <div className="settings-popover" role="menu">
          <div className="settings-popover-header">
            <div>
              <strong>{t("Settings")}</strong>
              <span>{t("App preferences and safety controls")}</span>
            </div>
            <button
              type="button"
              className="settings-close"
              onClick={() => setOpen(false)}
              title={t("Close")}
            >
              <X size={15} />
            </button>
          </div>

          <section className="settings-section">
            <div className="settings-section-title">
              <Activity size={15} />
              {t("Execution gate")}
            </div>
            <div className={`settings-status ${gateUnavailable ? "unknown" : gatePaused ? "paused" : "active"}`}>
              {gateUnavailable ? (
                <CircleDashed size={16} />
              ) : gatePaused ? (
                <CircleDashed size={16} />
              ) : (
                <CheckCircle2 size={16} />
              )}
              <span>
                {gateUnavailable ? t("Gate unavailable") : gatePaused ? t("Gate paused") : t("Gate active")}
              </span>
            </div>
            <div className="settings-metadata">
              {t("Last checked: {time}", { time: checkedAtText })}
            </div>
            <button
              type="button"
              className={`settings-action ${gatePaused ? "resume" : "pause"}`}
              onClick={onGateToggle}
              disabled={gateBusy || gateStatus === null}
              title={gatePaused ? t("Resume execution gate") : t("Pause execution gate")}
            >
              {gatePaused ? <PlayCircle size={16} /> : <PauseCircle size={16} />}
              {gatePaused ? t("Resume") : t("Pause")}
            </button>
            <button
              type="button"
              className="settings-action refresh"
              onClick={handleGateRefresh}
              disabled={refreshBusy}
              title={t("Refresh gate status")}
            >
              <RefreshCw size={16} className={refreshBusy ? "spin" : ""} />
              {t("Refresh gate status")}
            </button>
          </section>

          <section className="settings-section">
            <div className="settings-section-title">{t("Language")}</div>
            <LanguageSwitcher />
          </section>

          <section className="settings-section">
            <div className="settings-section-title">{t("Daemon console")}</div>
            <div className="settings-url" title={consoleUrl}>{consoleUrl}</div>
            <button
              type="button"
              className="settings-row-button"
              onClick={openConsole}
            >
              <ExternalLink size={16} />
              <span>{t("Open Web Console")}</span>
            </button>
            <button
              type="button"
              className="settings-row-button"
              onClick={copyConsoleUrl}
            >
              <Clipboard size={16} />
              <span>
                {copyStatus === "copied"
                  ? t("Copied")
                  : copyStatus === "failed"
                    ? t("Copy failed")
                    : t("Copy console URL")}
              </span>
            </button>
          </section>

          <section className="settings-section">
            <div className="settings-section-title">{t("Setup")}</div>
            <button
              type="button"
              className="settings-row-button"
              onClick={() => {
                onImportConfig();
                setOpen(false);
              }}
            >
              <Upload size={16} />
              <span>{t("Import from ~/.ssh/config")}</span>
            </button>
            <button
              type="button"
              className="settings-row-button"
              onClick={() => {
                onOpenSetup();
                setOpen(false);
              }}
            >
              <Wand2 size={16} />
              <span>{t("Open setup wizard")}</span>
            </button>
          </section>

          <div className="settings-daemon">
            <Activity size={15} />
            {t("Local daemon embedded")}
          </div>
        </div>
      )}
    </div>
  );
}
