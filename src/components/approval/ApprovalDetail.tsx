import { Check, Copy } from "lucide-react";
import { useState } from "react";
import { useI18n } from "../../i18n";
import type { ApprovalRequest, RiskLevel } from "../../types";
import RiskBadge from "../RiskBadge";
import { Button } from "../ui/button";
import { cn } from "../../lib/utils";

type Props = {
  approval: ApprovalRequest | null;
  busy: boolean;
  onApprove: () => void;
  onReject: () => void;
  /** Plan §5 risk #4: no "edit then execute" backend — copy the command and
   * drop the operator into the Execution module instead. */
  onCopyAndExecute: () => void;
};

/** Per-plan §5 risk #5: no matched_rule/fragment data from the backend, so the
 * whole command row is tinted with the risk-family color (no span-level dye). */
const COMMAND_TINT: Record<RiskLevel, string> = {
  low: "text-risk-low",
  medium: "text-risk-medium",
  high: "text-risk-high",
  blocked: "text-risk-high",
};

function formatTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

/** v3 3.1: detail column — command block on canvas-0, mono, full-row risk
 * tint, then the three action buttons and the keyboard hint line. */
export default function ApprovalDetail({
  approval,
  busy,
  onApprove,
  onReject,
  onCopyAndExecute,
}: Props) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);

  if (!approval) {
    return (
      <div className="flex min-h-[220px] items-center justify-center rounded-xl border border-dashed border-line">
        <p className="text-sm text-faint">{t("Nothing awaiting approval")}</p>
      </div>
    );
  }

  function handleCopy() {
    navigator.clipboard
      .writeText(approval?.command ?? "")
      .then(() => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1500);
      })
      .catch(() => {
        setCopied(false);
      });
  }

  return (
    <div className="min-w-0 space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="font-mono text-sm font-semibold">{approval.host}</span>
        <RiskBadge level={approval.risk_level} />
        <span className="ml-auto font-mono text-[11px] text-faint">
          {t("Requested at")} {formatTime(approval.requested_at)}
        </span>
      </div>

      <div className="rounded-xl border border-line bg-canvas-0 p-4">
        <code
          className={cn(
            "block break-all font-mono text-[13px] leading-relaxed",
            COMMAND_TINT[approval.risk_level]
          )}
        >
          {approval.command}
        </code>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Button variant="outline" disabled={busy} onClick={onReject} className="text-risk-high">
          {t("Reject")}
        </Button>
        <Button variant="secondary" disabled={busy} onClick={onCopyAndExecute}>
          {copied ? <Check size={14} /> : <Copy size={14} />}
          {t("Copy command and go to Execution")}
        </Button>
        <Button disabled={busy} onClick={onApprove} className="ml-auto">
          {t("Approve and execute")}
        </Button>
      </div>

      <p className="font-mono text-[11px] text-faint">
        {t("Keyboard: ↑↓ select · ↵ review · ⌘↵ approve · ⌫ reject")}
      </p>
    </div>
  );
}
