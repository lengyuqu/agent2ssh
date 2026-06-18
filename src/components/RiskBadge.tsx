import type { RiskLevel } from "../types";
import { useI18n } from "../i18n";
import { cn } from "../lib/utils";

type Props = {
  level: RiskLevel;
  hideLow?: boolean;
};

const map: Record<RiskLevel, string> = {
  low: "bg-success/12 text-success",
  medium: "bg-warning/15 text-warning",
  high: "bg-destructive/12 text-destructive",
  blocked: "bg-destructive text-destructive-foreground",
};

export default function RiskBadge({ level, hideLow }: Props) {
  const { t } = useI18n();
  if (hideLow && level === "low") return null;
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-full px-2 py-0.5 text-[11px] font-bold uppercase tracking-wide align-middle",
        map[level]
      )}
    >
      {t(level)}
    </span>
  );
}
