import { useI18n } from "../../i18n";
import type { RiskLevel } from "../../types";
import { cn } from "../../lib/utils";
import { formatClockTime } from "../../lib/format";

type Props = {
  items: ApprovalRequestLite[];
  selectedIndex: number;
  onSelect: (index: number) => void;
  /** Current clock in ms, ticked while pending items exist so TTL countdown stays live. */
  nowMs?: number;
  /** Override empty message when items are hidden by a filter rather than empty overall. */
  emptyMessage?: string;
};

/** Minimal shape used by the queue (subset of ApprovalRequest). */
export type ApprovalRequestLite = {
  id: string;
  host: string;
  command: string;
  risk_level: RiskLevel;
  requested_at: string;
  ttl_secs?: number;
  context?: { source?: string | null } | null;
};

/** Remaining seconds until this approval times out, or null when unparseable / unset. */
export function remainingTtlSecs(
  requestedAt: string,
  ttlSecs: number | undefined,
  nowMs: number,
): number | null {
  if (typeof ttlSecs !== "number" || !Number.isFinite(ttlSecs) || ttlSecs <= 0) return null;
  const started = new Date(requestedAt).getTime();
  if (!Number.isFinite(started)) return null;
  return Math.max(0, Math.ceil((started + ttlSecs * 1000 - nowMs) / 1000));
}

function formatTtl(secs: number): string {
  const minutes = Math.floor(secs / 60);
  const rem = secs % 60;
  return minutes > 0 ? `${minutes}m ${String(rem).padStart(2, "0")}s` : `${rem}s`;
}

/** Per-plan §4.2: the left edge carries the risk-family color. blocked reuses high. */
const RISK_EDGE: Record<RiskLevel, string> = {
  low: "border-l-risk-low",
  medium: "border-l-risk-medium",
  high: "border-l-risk-high",
  blocked: "border-l-risk-high",
};

/** v3 3.1: the 250px pending queue. Selected card = canvas-2 + 2px risk-colored
 * left edge; unselected cards are dimmed to text-2. Cards are buttons so ↑↓
 * selection can move real DOM focus (first card carries a stable id for the
 * global ⌘⇧A "jump to workbench" shortcut). */
export default function ApprovalQueue({
  items,
  selectedIndex,
  onSelect,
  nowMs = Date.now(),
  emptyMessage,
}: Props) {
  const { t } = useI18n();

  return (
    <div
      className="grid gap-2"
      role="listbox"
      aria-label={t("Approval queue")}
    >
      {items.map((item, index) => {
        const selected = index === selectedIndex;
        const remaining = remainingTtlSecs(item.requested_at, item.ttl_secs, nowMs);
        const urgent = remaining !== null && remaining <= 60;
        return (
          <button
            key={item.id}
            id={`approval-queue-card-${index}`}
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
                {formatClockTime(item.requested_at)}
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
            {(item.context?.source || remaining !== null) && (
              <div className="mt-1.5 flex items-center gap-2 font-mono text-[11px] text-faint">
                {item.context?.source && (
                  <span className="min-w-0 truncate">
                    {t("From")} {item.context.source}
                  </span>
                )}
                {remaining !== null && (
                  <span
                    className={cn(
                      "ml-auto shrink-0 tabular-nums",
                      urgent && "font-semibold text-risk-high",
                    )}
                  >
                    {remaining === 0 ? t("Expiring") : formatTtl(remaining)}
                  </span>
                )}
              </div>
            )}
          </button>
        );
      })}
      {items.length === 0 && (
        <div className="rounded-lg border border-dashed border-line px-3 py-6 text-center text-xs text-faint">
          {emptyMessage ?? t("Nothing awaiting approval")}
        </div>
      )}
    </div>
  );
}
