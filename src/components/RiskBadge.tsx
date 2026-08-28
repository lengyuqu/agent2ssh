import type { RiskLevel } from "../types";
import { useI18n } from "../i18n";
import { cn } from "../lib/utils";

type Props = {
  level: RiskLevel;
  hideLow?: boolean;
};

/* v3 §4.2: RiskBadge is the SINGLE implementation point for risk colors.
   low/medium/high = tinted bg + colored text; blocked = solid risk-high with
   canvas-0 text. Chips are data → mono uppercase. */
const map: Record<RiskLevel, string> = {
  low: "bg-risk-low/12 text-risk-low",
  medium: "bg-risk-medium/15 text-risk-medium",
  high: "bg-risk-high/12 text-risk-high",
  blocked: "bg-risk-high text-canvas-0",
};

/** Text-only risk tint for mono command rows (palette / detail / audit).
 *  Derived from the same family mapping above — RiskBadge stays the single
 *  source of risk-color truth (v3 §4.2). */
export const RISK_TEXT_CLS: Record<RiskLevel, string> = {
  low: "text-risk-low",
  medium: "text-risk-medium",
  high: "text-risk-high",
  blocked: "text-risk-high",
};

export default function RiskBadge({ level, hideLow }: Props) {
  const { t } = useI18n();
  if (hideLow && level === "low") return null;
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-full px-2 py-0.5 font-mono text-[11px] font-bold uppercase tracking-wide align-middle",
        map[level]
      )}
    >
      {t(level)}
    </span>
  );
}
