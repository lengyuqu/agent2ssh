import { Search, type LucideIcon } from "lucide-react";
import { type KeyboardEvent, useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { cn } from "../lib/utils";
import type { HostProfile } from "../types";
import { Input } from "./ui/input";

export type CommandPaletteModule = {
  id: string;
  label: string;
  icon: LucideIcon;
};

type PaletteResult =
  | { kind: "module"; id: string; label: string; icon: CommandPaletteModule["icon"] }
  | { kind: "host"; name: string; subtitle: string };

type CommandPaletteProps = {
  open: boolean;
  onClose: () => void;
  modules: readonly CommandPaletteModule[];
  hosts: HostProfile[];
  onNavigateModule: (id: string) => void;
  onSelectHost: (name: string) => void;
};

/** V1-3: global Ctrl+K search over modules and hosts (name/tags/user/group). */
export default function CommandPalette({
  open,
  onClose,
  modules,
  hosts,
  onNavigateModule,
  onSelectHost,
}: CommandPaletteProps) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setActiveIndex(0);
    const id = window.setTimeout(() => inputRef.current?.focus(), 0);
    return () => window.clearTimeout(id);
  }, [open]);

  const results = useMemo<PaletteResult[]>(() => {
    const q = query.trim().toLowerCase();

    const moduleResults: PaletteResult[] = modules
      .filter((m) => !q || t(m.label).toLowerCase().includes(q) || m.id.includes(q))
      .map((m) => ({ kind: "module" as const, id: m.id, label: m.label, icon: m.icon }));

    const hostResults: PaletteResult[] = hosts
      .filter((h) => {
        if (!q) return true;
        const haystack = [h.name, h.host, h.user ?? "", h.group, h.role ?? "", h.owner ?? "", ...(h.tags ?? [])]
          .join(" ")
          .toLowerCase();
        return haystack.includes(q);
      })
      .map((h) => ({
        kind: "host" as const,
        name: h.name,
        subtitle: `${h.user ? `${h.user}@` : ""}${h.host}${h.tags?.length ? ` · ${h.tags.join(", ")}` : ""}`,
      }));

    return [...moduleResults, ...hostResults].slice(0, 30);
  }, [query, modules, hosts, t]);

  useEffect(() => {
    setActiveIndex((idx) => Math.min(idx, Math.max(results.length - 1, 0)));
  }, [results.length]);

  if (!open) return null;

  function activate(result: PaletteResult) {
    if (result.kind === "module") {
      onNavigateModule(result.id);
    } else {
      onNavigateModule("hosts");
      onSelectHost(result.name);
    }
    onClose();
  }

  function handleKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIndex((i) => Math.min(i + 1, results.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const result = results[activeIndex];
      if (result) activate(result);
    }
  }

  return (
    <div
      className="fixed inset-0 z-[1200] flex items-start justify-center bg-black/50 p-4 pt-[12vh] max-sm:items-stretch max-sm:p-0"
      onClick={onClose}
    >
      <div
        className="w-full max-w-lg overflow-hidden rounded-xl border border-border bg-card text-card-foreground shadow-2xl max-sm:h-full max-sm:max-w-full max-sm:rounded-none max-sm:border-none"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2 border-b border-border px-4 py-3">
          <Search size={16} className="shrink-0 text-muted-foreground" />
          <Input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={t("Search modules, hosts, tags...")}
            className="h-8 border-none bg-transparent px-0 shadow-none focus-visible:ring-0"
          />
          <kbd className="shrink-0 rounded border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground">
            Esc
          </kbd>
        </div>
        <div className="max-h-[50vh] overflow-y-auto py-1 max-sm:max-h-[calc(100%-53px)]">
          {results.length === 0 && (
            <div className="px-4 py-6 text-center text-sm text-muted-foreground">
              {t("No matches")}
            </div>
          )}
          {results.map((result, index) => {
            const active = index === activeIndex;
            const key = result.kind === "module" ? `module-${result.id}` : `host-${result.name}`;
            return (
              <button
                key={key}
                type="button"
                className={cn(
                  "flex w-full items-center gap-3 px-4 py-2 text-left text-sm",
                  active ? "bg-muted text-foreground" : "text-foreground/90 hover:bg-muted/60"
                )}
                onMouseEnter={() => setActiveIndex(index)}
                onClick={() => activate(result)}
              >
                {result.kind === "module" ? (
                  <>
                    <result.icon size={15} className="shrink-0 text-muted-foreground" />
                    <span className="truncate">{t(result.label)}</span>
                    <span className="ml-auto shrink-0 text-[10px] uppercase text-muted-foreground/60">
                      {t("Module")}
                    </span>
                  </>
                ) : (
                  <>
                    <span className="flex size-[15px] shrink-0 items-center justify-center rounded-sm bg-muted text-[9px] font-bold text-muted-foreground">
                      {result.name.slice(0, 1).toUpperCase()}
                    </span>
                    <span className="min-w-0 flex-1 truncate">
                      {result.name}
                      <span className="ml-2 text-xs text-muted-foreground">{result.subtitle}</span>
                    </span>
                    <span className="ml-auto shrink-0 text-[10px] uppercase text-muted-foreground/60">
                      {t("Host")}
                    </span>
                  </>
                )}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}
