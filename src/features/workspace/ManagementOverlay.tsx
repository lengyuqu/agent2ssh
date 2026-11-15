import { useState } from "react";
import { X } from "lucide-react";
import { useI18n } from "../../i18n";
import { MODULES, SIDEBAR_GROUPS } from "../../nav";
import { Dialog } from "../../components/ui/dialog";

type Props = {
  onClose: () => void;
  onNavigate: (id: (typeof MODULES)[number]["id"]) => void;
};

/**
 * The management surface: every governance/configuration module that used to
 * live in the sidebar, reachable as a grouped grid. Picking one closes the
 * workspace view and renders the module page; the tab bar returns via
 * "Back to workspace".
 */
export default function ManagementOverlay({ onClose, onNavigate }: Props) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const needle = query.trim().toLowerCase();

  return (
    <Dialog onClose={onClose} className="max-w-2xl">
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-lg font-bold">{t("Management")}</h3>
        <button
          type="button"
          className="rounded-md p-1 text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={onClose}
          title={t("Close")}
        >
          <X size={18} />
        </button>
      </div>
      <input
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        placeholder={t("Search hosts")}
        className="mt-3 w-full rounded-md border border-input bg-background px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-ring"
      />
      <div className="mt-4 grid max-h-[60vh] gap-4 overflow-y-auto">
        {SIDEBAR_GROUPS.map((group) => {
          const items = group.modules
            .map((id) => MODULES.find((module) => module.id === id) ?? MODULES[0])
            .filter((module) => !needle || t(module.label).toLowerCase().includes(needle));
          if (items.length === 0) return null;
          return (
            <div key={group.title || "pinned"}>
              {group.title && (
                <div className="mb-1.5 text-[10px] font-bold uppercase tracking-wide text-muted-foreground">
                  {t(group.title)}
                </div>
              )}
              <div className="grid grid-cols-2 gap-1.5 max-sm:grid-cols-1">
                {items.map((module) => {
                  const { label, icon: Icon } = module;
                  return (
                    <button
                      key={module.id}
                      type="button"
                      className="flex items-center gap-2.5 rounded-lg border border-border px-3 py-2 text-left text-sm text-foreground transition-colors hover:border-ring/60 hover:bg-muted"
                      onClick={() => onNavigate(module.id)}
                    >
                      <Icon size={15} className="shrink-0 text-muted-foreground" />
                      <span className="min-w-0 truncate">{t(label)}</span>
                    </button>
                  );
                })}
              </div>
            </div>
          );
        })}
      </div>
    </Dialog>
  );
}
