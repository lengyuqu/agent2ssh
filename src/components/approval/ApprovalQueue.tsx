import { useI18n } from "../../i18n";
import type { RiskLevel } from "../../types";
import { cn } from "../../lib/utils";

type Props = {
  items: ApprovalRequestLite[];
  selectedIndex: number;
  onSelect: (index: number) => void;
};

/** Minimal shape used by the queue (subset of ApprovalRequest). */
export type ApprovalRequestLite = {
  id: string;
  host: string;
  command: string;
  risk_level: RiskLevel;
  requested_at: string;
};

/** Per-plan §4.2: the left edge carries the risk-family color. blocked reuses high. */
const RISK_EDGE: Record<RiskLevel, string> = {
  low: "border-l-risk-low",
  medium: "border-l-risk-medium",
  high: "border-l-risk-high",
  blocked: "border-l-risk-high",
};

function formatTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleTimeString();
}

/** v3 3.1: the 250px pending queue. Selected card = canvas-2 + 2px risk-colored
 * left edge; unselected cards are dimmed to text-2. Cards are buttons so ↑↓
 * selection can move real DOM focus (first card carries a stable id for the
 * global ⌘⇧A "jump to workbench" shortcut). */
export default function ApprovalQueue({ items, selectedIndex, onSelect }: Props) {
  const { t } = useI18n();

  return (
    <div
      className="grid gap-2"
      role="listbox"
      aria-label={t("Approval queue")}
    >
      {items.map((item, index) => {
        const selected = index === selectedIndex;
        return (
          <button
            key={item.id}
            id={index === 0 ? "approval-queue-card-0" : undefined}
            type="button"
            role="option"
            aria-selected={selected}
            onClick={() => onSelect(index)}
            className={cn(
              "w-full rounded-lg border border-line border-l-2 p-3 text-left transition-colors",
              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
              RISK_EDGE[item.risk_level],
              selected
                ? "bg-canvas-2 text-foreground"
                : "bg-transparent text-muted-foreground hover:bg-canvas-2/60"
            )}
          >
            <div className="flex items-center gap-2">
              <span className="truncate font-mono text-xs font-semibold">{item.host}</span>
              <span className="ml-auto shrink-0 font-mono text-[11px] text-faint">
                {formatTime(item.requested_at)}
              </span>
            </div>
            <code
              className={cn(
                "mt-1.5 block break-all font-mono text-xs leading-relaxed",
                selected ? "line-clamp-2" : "line-clamp-1 opacity-80"
              )}
            >
              {item.command}
            </code>
          </button>
        );
      })}
      {items.length === 0 && (
        <div className="rounded-lg border border-dashed border-line px-3 py-6 text-center text-xs text-faint">
          {t("Nothing awaiting approval")}
        </div>
      )}
    </div>
  );
}
