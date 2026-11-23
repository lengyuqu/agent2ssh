import { X } from "lucide-react";
import { useEffect, type ReactNode } from "react";
import { useI18n } from "../i18n";
import { IconButton } from "./ui/icon-button";

type HostDrawerProps = {
  open: boolean;
  onClose: () => void;
  /** Set while editing an existing host. Omitted (or null) while creating one. */
  editingHostName?: string | null;
  children: ReactNode;
};

/** Right slide-over for add/edit host. The form stays owned by App via children. */
export default function HostDrawer({
  open,
  onClose,
  editingHostName,
  children,
}: HostDrawerProps) {
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

  const title = editingHostName
    ? t("Edit host: {name}", { name: editingHostName })
    : t("Add Host");

  return (
    <div className="fixed inset-0 z-[1000]">
      <div
        data-testid="host-drawer-overlay"
        className="absolute inset-0 bg-black/50 backdrop-blur-xs"
        onClick={onClose}
      />
      <aside
        role="dialog"
        aria-modal="true"
        aria-labelledby="host-drawer-title"
        className="absolute inset-y-0 right-0 flex w-[460px] max-w-full translate-x-0 flex-col border-l border-border bg-card transition-transform duration-200 ease-out starting:translate-x-full"
      >
        <div className="flex shrink-0 items-center gap-3 border-b border-border px-4 py-3">
          <h2
            id="host-drawer-title"
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
