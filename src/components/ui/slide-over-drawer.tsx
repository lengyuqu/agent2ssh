import { X } from "lucide-react";
import { useEffect, type ReactNode } from "react";
import { useI18n } from "../../i18n";
import { cn } from "../../lib/utils";
import { IconButton } from "./icon-button";

type SlideOverDrawerProps = {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** Tailwind width for the panel. Defaults to the host drawer's 460px. */
  widthClassName?: string;
  /** Overlay test id. HostDrawer keeps `host-drawer-overlay` so its tests still click it. */
  overlayTestId?: string;
  /** Id of the heading. Callers that already labelled the dialog keep their id. */
  titleId?: string;
};

/**
 * Right-hand slide-over: overlay, slide-in, Escape, close button, title, body.
 *
 * Host edit used to own this shell. Tunnels and proxies open the same drawer
 * for create and edit, so the page can stay a full-width list until then.
 */
export function SlideOverDrawer({
  open,
  title,
  onClose,
  children,
  widthClassName = "w-[460px]",
  overlayTestId = "slide-over-overlay",
  titleId = "slide-over-title",
}: SlideOverDrawerProps) {
  const { t } = useI18n();

  useEffect(() => {
    if (!open) return;
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onClose();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-[1000]">
      <div
        data-testid={overlayTestId}
        className="absolute inset-0 bg-black/50 backdrop-blur-xs"
        onClick={onClose}
      />
      <aside
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className={cn(
          "absolute inset-y-0 right-0 flex max-w-full translate-x-0 flex-col border-l border-border bg-card transition-transform duration-200 ease-out starting:translate-x-full",
          widthClassName,
        )}
      >
        <div className="flex shrink-0 items-center gap-3 border-b border-border px-4 py-3">
          <h2
            id={titleId}
            className="min-w-0 flex-1 truncate text-base font-semibold text-foreground"
          >
            {title}
          </h2>
          <IconButton onClick={onClose} title={t("Close")}>
            <X size={16} />
          </IconButton>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto p-4">{children}</div>
      </aside>
    </div>
  );
}
