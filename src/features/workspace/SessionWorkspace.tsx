import { useMemo } from "react";
import { useTheme, type Theme as AppTheme } from "../../theme";
import { useI18n } from "../../i18n";
import {
  TERMINAL_THEME_STORAGE_KEY,
  isTerminalThemeId,
  terminalThemeBackground,
  type TerminalThemeId,
} from "../../terminalThemes";
import TerminalView from "../../components/TerminalView";
import SFTPPanel from "../../components/SFTPPanel";
import type { HostProfile } from "../../types";
import type { WorkspaceTab } from "./types";

type Props = {
  tab: WorkspaceTab;
  hosts: HostProfile[];
  fileDrawerOpen: boolean;
};

function initialTerminalTheme(): TerminalThemeId {
  try {
    const saved = localStorage.getItem(TERMINAL_THEME_STORAGE_KEY);
    if (saved && isTerminalThemeId(saved)) return saved;
  } catch {
    // Opaque origin — fall through to the default.
  }
  return "app";
}

function systemTheme(): AppTheme {
  if (typeof window === "undefined") return "dark";
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

/**
 * One session workspace: the host's terminal on top, the same host's SFTP
 * browser in a collapsible bottom drawer. The file panel receives a one-host
 * list so its own selector cannot drift away from the tab's connection
 * context — the pairing is the whole point of the workspace model.
 */
export default function SessionWorkspace({ tab, hosts, fileDrawerOpen }: Props) {
  const { t } = useI18n();
  const { theme: appThemeRaw } = useTheme();

  const host = useMemo(() => hosts.find((h) => h.name === tab.host), [hosts, tab.host]);
  const terminalTheme = useMemo(initialTerminalTheme, []);
  const effectiveAppTheme: AppTheme =
    appThemeRaw === "system" ? systemTheme() : appThemeRaw;
  const terminalBackground = useMemo(
    () => terminalThemeBackground(terminalTheme, effectiveAppTheme),
    [effectiveAppTheme, terminalTheme],
  );

  const tabHosts = useMemo(
    () => hosts.filter((h) => h.name === tab.host),
    [hosts, tab.host],
  );

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div
        className="min-h-0 flex-1 [&_.xterm]:h-full [&_.xterm-viewport]:h-full"
        style={{ background: terminalBackground }}
      >
        <TerminalView host={tab.host} terminalTheme={terminalTheme} appTheme={effectiveAppTheme} />
      </div>
      {fileDrawerOpen && (
        <div className="h-[45%] shrink-0 border-t border-line">
          {host ? (
            <SFTPPanel hosts={tabHosts} initialHost={tab.host} />
          ) : (
            <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
              {t("Host no longer exists")}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
