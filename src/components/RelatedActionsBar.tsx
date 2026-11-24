import { memo } from "react";
import { Zap, type LucideIcon } from "lucide-react";
import { useI18n } from "../i18n";
import { cn } from "../lib/utils";

export type RelatedModule = { id: string; label: string; icon: LucideIcon };

type Props = {
  current: RelatedModule;
  related: RelatedModule[];
  onNavigate: (id: string) => void;
  className?: string;
};

/** Jumps to the modules an operator is likely to open next. The current module
 *  stays selected in App (including the host); this bar only switches modules. */
function RelatedActionsBar({ related, onNavigate, className }: Props) {
  const { t } = useI18n();
  if (related.length === 0) return null;
  return (
    <nav
      aria-label={t("Related modules")}
      className={cn("flex flex-wrap items-center gap-1.5 text-sm", className)}
    >
      <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
        <Zap size={13} />
        {t("Quick jump")}
      </span>
      {related.map((module) => (
        <button
          key={module.id}
          type="button"
          onClick={() => onNavigate(module.id)}
          className="inline-flex appearance-none items-center gap-1.5 rounded-full border border-border bg-transparent px-2.5 py-0.5 text-muted-foreground transition-colors hover:border-primary hover:text-primary"
        >
          <module.icon size={13} />
          {t(module.label)}
        </button>
      ))}
    </nav>
  );
}

export default memo(RelatedActionsBar);
