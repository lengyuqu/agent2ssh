import type { RiskLevel } from "../types";
import { useI18n } from "../i18n";

type Props = {
  level: RiskLevel;
  hideLow?: boolean;
};

const map: Record<RiskLevel, { label: string; cls: string }> = {
  low: { label: "low", cls: "risk-low" },
  medium: { label: "medium", cls: "risk-medium" },
  high: { label: "high", cls: "risk-high" },
  blocked: { label: "blocked", cls: "risk-blocked" },
};

export default function RiskBadge({ level, hideLow }: Props) {
  const { t } = useI18n();
  if (hideLow && level === "low") return null;
  const { label, cls } = map[level];
  return <span className={`risk-badge ${cls}`}>{t(label)}</span>;
}
