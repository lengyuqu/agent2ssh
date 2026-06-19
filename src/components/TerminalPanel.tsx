import { Plus, TerminalSquare, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useI18n } from "../i18n";
import type { HostProfile } from "../types";
import {
  isTerminalThemeId,
  TERMINAL_THEME_OPTIONS,
  TERMINAL_THEME_STORAGE_KEY,
  terminalThemeBackground,
  type TerminalThemeId,
} from "../terminalThemes";
import { useTheme, type Theme as AppTheme } from "../theme";
import TerminalView from "./TerminalView";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { Select } from "./ui/select";
import { cn } from "../lib/utils";

type Props = {
  hosts: HostProfile[];
  initialHost?: string;
};

type Tab = { id: string; host: string };

let counter = 0;
function nextId(): string {
  counter += 1;
  return `term-${counter}-${Date.now()}`;
}

function initialTerminalTheme(): TerminalThemeId {
  try {
    const saved = localStorage.getItem(TERMINAL_THEME_STORAGE_KEY);
    if (saved && isTerminalThemeId(saved)) return saved;
  } catch {
    // localStorage may be unavailable
  }
  return "app";
}

function systemTheme(): AppTheme {
  if (typeof window === "undefined") return "dark";
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

export default function TerminalPanel({ hosts, initialHost = "" }: Props) {
  const { t } = useI18n();
  const { theme: appTheme } = useTheme();
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [newHost, setNewHost] = useState(initialHost || hosts[0]?.name || "");
  const [terminalTheme, setTerminalTheme] = useState<TerminalThemeId>(() => initialTerminalTheme());
  const [resolvedSystemTheme, setResolvedSystemTheme] = useState<AppTheme>(() => systemTheme());

  useEffect(() => {
    try {
      localStorage.setItem(TERMINAL_THEME_STORAGE_KEY, terminalTheme);
    } catch {
      // ignore persistence failures
    }
  }, [terminalTheme]);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: light)");
    const update = () => setResolvedSystemTheme(systemTheme());
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);

  const effectiveAppTheme = appTheme === "system" ? resolvedSystemTheme : appTheme;
  const terminalBackground = useMemo(
    () => terminalThemeBackground(terminalTheme, effectiveAppTheme),
    [effectiveAppTheme, terminalTheme]
  );

  function openTab() {
    if (!newHost) return;
    const id = nextId();
    setTabs((prev) => [...prev, { id, host: newHost }]);
    setActiveId(id);
  }

  function closeTab(id: string) {
    setTabs((prev) => {
      const next = prev.filter((tab) => tab.id !== id);
      setActiveId((current) =>
        current === id ? (next.length > 0 ? next[next.length - 1].id : null) : current
      );
      return next;
    });
  }

  return (
    <Card className="flex h-[72vh] flex-col overflow-hidden p-0">
      <div className="flex items-center gap-1 overflow-x-auto border-b border-border bg-muted/40 px-2 py-1.5">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveId(tab.id)}
            className={cn(
              "inline-flex shrink-0 items-center gap-1.5 rounded-md border px-2.5 py-1 text-xs font-medium transition-colors",
              tab.id === activeId
                ? "border-primary/40 bg-primary/10 text-primary"
                : "border-transparent text-muted-foreground hover:bg-muted"
            )}
          >
            <TerminalSquare size={12} />
            <span className="max-w-[140px] truncate">{tab.host}</span>
            <span
              role="button"
              tabIndex={-1}
              onClick={(e) => {
                e.stopPropagation();
                closeTab(tab.id);
              }}
              className="rounded p-0.5 hover:bg-black/10 dark:hover:bg-white/10"
              aria-label={t("Close")}
            >
              <X size={11} />
            </span>
          </button>
        ))}
        <div className="ml-auto flex shrink-0 items-center gap-1.5">
          <label className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
            <span>{t("Terminal theme")}</span>
            <Select
              value={terminalTheme}
              onChange={(e) => {
                const next = e.target.value;
                if (isTerminalThemeId(next)) setTerminalTheme(next);
              }}
              className="h-7 w-[170px] text-xs"
            >
              {TERMINAL_THEME_OPTIONS.map((option) => (
                <option key={option.id} value={option.id}>
                  {t(option.label)}
                </option>
              ))}
            </Select>
          </label>
          <Select
            value={newHost}
            onChange={(e) => setNewHost(e.target.value)}
            className="h-7 w-[150px] text-xs"
          >
            {hosts.length === 0 && <option value="">{t("No hosts")}</option>}
            {hosts.map((host) => (
              <option key={host.name} value={host.name}>
                {host.name}
              </option>
            ))}
          </Select>
          <Button size="sm" onClick={openTab} disabled={!newHost} className="h-7">
            <Plus size={13} />
            {t("New terminal")}
          </Button>
        </div>
      </div>

      <div className="relative flex-1" style={{ backgroundColor: terminalBackground }}>
        {tabs.length === 0 && (
          <div className="absolute inset-0 flex items-center justify-center text-sm text-white/60">
            {t("Open a terminal to a host to get started.")}
          </div>
        )}
        {tabs.map((tab) => (
          <div
            key={tab.id}
            className="absolute inset-0"
            style={{
              visibility: tab.id === activeId ? "visible" : "hidden",
              zIndex: tab.id === activeId ? 1 : 0,
            }}
          >
            <TerminalView
              host={tab.host}
              terminalTheme={terminalTheme}
              appTheme={effectiveAppTheme}
            />
          </div>
        ))}
      </div>
    </Card>
  );
}
