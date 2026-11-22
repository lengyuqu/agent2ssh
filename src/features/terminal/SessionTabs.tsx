import { BookMarked, FolderOpen, Maximize2, Minimize2, Plus, RadioTower, X } from "lucide-react";
import type { ReactNode } from "react";
import { useMemo } from "react";
import { useI18n } from "../../i18n";
import { cn } from "../../lib/utils";
import { sessionLabels, type TerminalSession } from "./sessions";

type Props = {
  sessions: TerminalSession[];
  panes: Array<string | null>;
  connectedHosts: ReadonlySet<string>;
  focusMode: boolean;
  filesOpen: boolean;
  canBroadcast: boolean;
  onActivate: (id: string) => void;
  onClose: (id: string) => void;
  onReorder: (dragId: string, overId: string) => void;
  onNewSession: () => void;
  onOpenSnippets: () => void;
  onOpenBroadcast: () => void;
  onToggleFiles: () => void;
  onToggleFocusMode: () => void;
};

const DRAG_TYPE = "text/terminal-session";

/**
 * The session strip: one tab per open terminal session, draggable to reorder,
 * each showing which pane it is on screen in. The actions on the right stay
 * reachable in focus mode, when the host column is hidden — snippets,
 * broadcast, the file drawer and focus itself.
 */
export default function SessionTabs({
  sessions,
  panes,
  connectedHosts,
  focusMode,
  filesOpen,
  canBroadcast,
  onActivate,
  onClose,
  onReorder,
  onNewSession,
  onOpenSnippets,
  onOpenBroadcast,
  onToggleFiles,
  onToggleFocusMode,
}: Props) {
  const { t } = useI18n();
  const labels = useMemo(() => sessionLabels(sessions), [sessions]);

  return (
    <div className="flex shrink-0 items-center gap-1 border-b border-line bg-muted/40 px-2 py-1.5">
      <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto">
        {sessions.map((session) => {
          const paneIndex = panes.indexOf(session.id);
          const onScreen = paneIndex >= 0;
          const isConnected = connectedHosts.has(session.host);
          const label = labels.get(session.id) ?? session.host;
          return (
            <div
              key={session.id}
              className={cn(
                "group flex shrink-0 items-center gap-1.5 rounded-lg px-2 py-1 text-xs transition-colors",
                onScreen
                  ? "bg-primary/15 text-foreground"
                  : "text-muted-foreground hover:bg-muted hover:text-foreground",
              )}
              draggable
              onDragStart={(event) => {
                event.dataTransfer.setData(DRAG_TYPE, session.id);
                event.dataTransfer.effectAllowed = "move";
              }}
              onDragOver={(event) => {
                if (event.dataTransfer.types.includes(DRAG_TYPE)) event.preventDefault();
              }}
              onDrop={(event) => {
                const dragId = event.dataTransfer.getData(DRAG_TYPE);
                if (dragId && dragId !== session.id) onReorder(dragId, session.id);
              }}
            >
              <button
                type="button"
                className="flex items-center gap-1.5"
                onClick={() => onActivate(session.id)}
                title={label}
              >
                <span
                  className={cn(
                    "size-1.5 shrink-0 rounded-full",
                    isConnected ? "bg-success" : "bg-risk-medium/70",
                  )}
                />
                <span className="max-w-40 truncate font-medium">{label}</span>
                {onScreen && (
                  <span className="rounded bg-primary/20 px-1 font-mono text-[10px] font-bold text-primary">
                    {paneIndex + 1}
                  </span>
                )}
              </button>
              <button
                type="button"
                className="rounded p-0.5 text-muted-foreground opacity-0 transition-opacity hover:bg-foreground/10 hover:text-foreground focus-visible:opacity-100 group-hover:opacity-100"
                onClick={() => onClose(session.id)}
                title={t("Close")}
                aria-label={t("Close session to {host}", { host: label })}
              >
                <X size={12} />
              </button>
            </div>
          );
        })}
        {sessions.length === 0 && (
          <span className="px-2 text-xs text-muted-foreground">{t("No active sessions")}</span>
        )}
        <button
          type="button"
          onClick={onNewSession}
          title={t("New terminal")}
          aria-label={t("New terminal")}
          className="shrink-0 rounded-lg p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
        >
          <Plus size={13} />
        </button>
      </div>

      <StripAction label={t("Command snippets")} onClick={onOpenSnippets}>
        <BookMarked size={13} />
      </StripAction>
      <StripAction
        label={t("Broadcast command")}
        onClick={onOpenBroadcast}
        disabled={!canBroadcast}
      >
        <RadioTower size={13} />
      </StripAction>
      <StripAction
        label={t(filesOpen ? "Hide files" : "Show files")}
        onClick={onToggleFiles}
        active={filesOpen}
        disabled={sessions.length === 0}
      >
        <FolderOpen size={13} />
        <span className="max-lg:hidden">{t("Files")}</span>
      </StripAction>
      <StripAction
        label={t("Focus mode")}
        onClick={onToggleFocusMode}
        active={focusMode}
      >
        {focusMode ? <Minimize2 size={13} /> : <Maximize2 size={13} />}
      </StripAction>
    </div>
  );
}

function StripAction({
  label,
  onClick,
  active = false,
  disabled = false,
  children,
}: {
  label: string;
  onClick: () => void;
  active?: boolean;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={label}
      aria-label={label}
      aria-pressed={active}
      className={cn(
        "flex shrink-0 items-center gap-1 rounded-lg px-2 py-1 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-40",
        active
          ? "bg-primary/15 text-primary"
          : "text-muted-foreground hover:bg-muted hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}