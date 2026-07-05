import { ChevronRight, type LucideIcon } from "lucide-react";
import { useI18n } from "../i18n";

export type BreadcrumbModule = { id: string; label: string; icon: LucideIcon };

type Props = {
  current: BreadcrumbModule;
  related: BreadcrumbModule[];
  onNavigate: (id: string) => void;
};

/** V3-4: mesh navigation between related modules (Host→Exec, Host→Audit, Approvals→Exec, …),
 *  not a linear tab trail — there is no "back", just jumps to whatever else is relevant here. */
export default function Breadcrumb({ current, related, onNavigate }: Props) {
  const { t } = useI18n();
  if (related.length === 0) return null;
  return (
    <nav aria-label={t("Related modules")} className="flex flex-wrap items-center gap-1.5 text-sm">
      <span className="inline-flex items-center gap-1.5 font-medium text-foreground">
        <current.icon size={14} className="text-muted-foreground" />
        {t(current.label)}
      </span>
      {related.map((module) => (
        <span key={module.id} className="inline-flex items-center gap-1.5">
          <ChevronRight size={13} className="text-muted-foreground/50" />
          <button
            type="button"
            onClick={() => onNavigate(module.id)}
            className="inline-flex items-center gap-1.5 rounded-md px-1.5 py-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <module.icon size={13} />
            {t(module.label)}
          </button>
        </span>
      ))}
    </nav>
  );
}
