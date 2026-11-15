import { Maximize2, Minimize2, X } from "lucide-react";
import { useI18n } from "../../i18n";
import { cn } from "../../lib/utils";
import type { WorkspaceTab } from "./types";

type Props = {
  tabs: WorkspaceTab[];
  activeTabId: string | null;
  connectedHosts: Set<string>;
  focusMode: boolean;
  fileDrawerOpen: boolean;
  onActivate: (id: string) => void;
  onClose: (id: string) => void;
  onReorder: (dragId: string, overId: string) => void;
  onToggleFocusMode: () => void;
  onToggleFileDrawer: () => void;
};

/**
 * The workspace tab bar: one tab per host session (draggable to reorder), a
 * focus-mode toggle (collapse sidebar + file drawer) and a per-tab file-drawer
 * toggle. Mirrors the connection-first model — tabs are sessions, not feature
 * modules.
 */
export default function WorkspaceTabBar({
  tabs,
  activeTabId,
  connectedHosts,
  focusMode,
  fileDrawerOpen,
  onActivate,
  onClose,
  onReorder,
  onToggleFocusMode,
  onToggleFileDrawer,
}: Props) {
  const { t } = useI18n();

  return (
    <div className="flex shrink-0 items-center gap-1 border-b border-line bg-sidebar/60 px-2 py-1.5">
      <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto">
        {tabs.map((tab) => {
          const active = tab.id === activeTabId;
          const isConnected = connectedHosts.has(tab.host);
          return (
            <div
              key={tab.id}
              className={cn(
                "group flex shrink-0 items-center gap-1.5 rounded-lg px-2.5 py-1 text-xs transition-colors",
                active
                  ? "bg-white/10 text-foreground"
                  : "text-sidebar-foreground/80 hover:bg-white/5 hover:text-sidebar-foreground"
              )}
              draggable
              onDragStart={(event) => {
                event.dataTransfer.setData("text/workspace-tab", tab.id);
                event.dataTransfer.effectAllowed = "move";
              }}
              onDragOver={(event) => {
                if (event.dataTransfer.types.includes("text/workspace-tab")) {
                  event.preventDefault();
                }
              }}
              onDrop={(event) => {
                const dragId = event.dataTransfer.getData("text/workspace-tab");
                if (dragId && dragId !== tab.id) onReorder(dragId, tab.id);
              }}
            >
              <button
                type="button"
                className="flex items-center gap-1.5"
                onClick={() => onActivate(tab.id)}
                title={tab.host}
              >
                <span
                  className={cn(
                    "size-1.5 shrink-0 rounded-full",
                    isConnected ? "bg-success" : "bg-risk-medium/70"
                  )}
                />
                <span className="max-w-40 truncate font-medium">{tab.host}</span>
              </button>
              <button
                type="button"
                className="rounded p-0.5 text-sidebar-foreground/50 opacity-0 transition-opacity hover:bg-white/10 hover:text-sidebar-foreground group-hover:opacity-100"
                onClick={() => onClose(tab.id)}
                title={t("Close")}
              >
                <X size={12} />
              </button>
            </div>
          );
        })}
        {tabs.length === 0 && (
          <span className="px-2 text-xs text-sidebar-foreground/45">
            {t("Workspace")}
          </span>
        )}
      </div>

      {tabs.length > 0 && (
        <button
          type="button"
          onClick={onToggleFileDrawer}
          title={t(fileDrawerOpen ? "Hide files" : "Show files")}
          className={cn(
            "flex shrink-0 items-center gap-1 rounded-lg px-2 py-1 text-xs transition-colors",
            fileDrawerOpen
              ? "bg-white/10 text-foreground"
              : "text-sidebar-foreground/70 hover:bg-white/5 hover:text-sidebar-foreground"
          )}
        >
          {t("Files")}
        </button>
      )}
      <button
        type="button"
        onClick={onToggleFocusMode}
        title={t("Focus mode")}
        className={cn(
          "flex shrink-0 items-center gap-1 rounded-lg p-1.5 text-xs transition-colors",
          focusMode
            ? "bg-white/10 text-foreground"
            : "text-sidebar-foreground/70 hover:bg-white/5 hover:text-sidebar-foreground"
        )}
      >
        {focusMode ? <Minimize2 size={13} /> : <Maximize2 size={13} />}
      </button>
    </div>
  );
}
