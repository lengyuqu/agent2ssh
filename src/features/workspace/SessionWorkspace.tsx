import { useMemo, useRef, useState } from "react";
import { ChevronDown, SquareTerminal } from "lucide-react";
import { useTheme, type Theme as AppTheme } from "../../theme";
import { useI18n } from "../../i18n";
import { api } from "../../api";
import {
  TERMINAL_THEME_STORAGE_KEY,
  isTerminalThemeId,
  terminalThemeBackground,
  type TerminalThemeId,
} from "../../terminalThemes";
import TerminalView, { type TerminalViewHandle } from "../../components/TerminalView";
import SFTPPanel from "../../components/SFTPPanel";
import type { HostProfile, Snippet } from "../../types";
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
 *
 * The terminal also shows a floating snippet bar and the reported working
 * directory when the shell supports OSC 7.
 */
export default function SessionWorkspace({ tab, hosts, fileDrawerOpen }: Props) {
  const { t } = useI18n();
  const { theme: appThemeRaw } = useTheme();
  const terminalRef = useRef<TerminalViewHandle>(null);

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

  // ── Snippet bar ──
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [snippetsOpen, setSnippetsOpen] = useState(false);
  function toggleSnippets() {
    if (!snippetsOpen && snippets.length === 0) {
      api
        .listSnippets()
        .then(setSnippets)
        .catch(() => setSnippets([]));
    }
    setSnippetsOpen((open) => !open);
  }
  function sendSnippet(command: string) {
    terminalRef.current?.sendText(command);
    terminalRef.current?.sendText("\r");
  }

  const [cwd, setCwd] = useState<string | null>(null);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div
        className="relative min-h-0 flex-1 [&_.xterm]:h-full [&_.xterm-viewport]:h-full"
        style={{ background: terminalBackground }}
      >
        <TerminalView
          ref={terminalRef}
          host={tab.host}
          terminalTheme={terminalTheme}
          appTheme={effectiveAppTheme}
          onOsc7Cwd={setCwd}
        />

        {/* OSC 7 cwd chip — best-effort: only shells with integration show it. */}
        {cwd && (
          <span
            className="absolute bottom-2 left-2 max-w-56 truncate rounded-lg border border-edge bg-popover/90 px-2 py-1 font-mono text-[11px] text-popover-foreground shadow"
            title={cwd}
          >
            {cwd}
          </span>
        )}

        {/* Floating snippet bar (FileTerm-style command templates). */}
        <div className="absolute right-2 top-2">
          <button
            type="button"
            className={cnFab(snippetsOpen)}
            onClick={toggleSnippets}
            title={t("Snippets")}
          >
            <SquareTerminal size={12} />
            {t("Snippets")}
            <ChevronDown
              size={11}
              className={cnChevron(snippetsOpen)}
            />
          </button>
          {snippetsOpen && (
            <div className="absolute right-0 top-full z-20 mt-1 max-h-64 w-72 overflow-y-auto rounded-lg border border-edge bg-popover p-1 text-popover-foreground shadow-lg">
              {snippets.length === 0 && (
                <div className="px-2.5 py-2 text-xs text-muted-foreground">
                  {t("No snippets yet")}
                </div>
              )}
              {snippets.map((snippet) => (
                <button
                  key={snippet.name}
                  type="button"
                  className="flex w-full flex-col items-start gap-0.5 rounded-md px-2.5 py-1.5 text-left transition-colors hover:bg-muted"
                  onClick={() => sendSnippet(snippet.command)}
                  title={snippet.command}
                >
                  <span className="text-xs font-medium">{snippet.name}</span>
                  <span className="max-w-full truncate font-mono text-[10px] text-muted-foreground">
                    {snippet.command}
                  </span>
                </button>
              ))}
            </div>
          )}
        </div>
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

function cnFab(active: boolean): string {
  return [
    "flex items-center gap-1 rounded-lg border px-2 py-1 text-[11px] shadow-sm transition-colors",
    active
      ? "border-ring/60 bg-popover text-popover-foreground"
      : "border-edge bg-popover/80 text-popover-foreground/80 hover:bg-popover hover:text-popover-foreground",
  ].join(" ");
}

function cnChevron(open: boolean): string {
  return ["transition-transform", open ? "rotate-180" : ""].join(" ");
}
