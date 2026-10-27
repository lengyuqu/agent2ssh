import { Check, Copy } from "lucide-react";
import { useState } from "react";
import { useI18n } from "../../i18n";
import type { ApprovalRequest, RiskLevel } from "../../types";
import RiskBadge from "../RiskBadge";
import { Button } from "../ui/button";
import { cn } from "../../lib/utils";
import { formatDateTime } from "../../lib/format";

type Props = {
  approval: ApprovalRequest | null;
  busy: boolean;
  onApprove: () => void;
  onReject: () => void;
  /** Plan §5 risk #4: no "edit then execute" backend — copy the command and
   * drop the operator into the Execution module instead. */
  onCopyAndExecute: () => void;
};

/** The backend reports which policy matched but not which fragment of the
 * command triggered it, so the whole command row is tinted with the
 * risk-family color (no span-level dye). */
const COMMAND_TINT: Record<RiskLevel, string> = {
  low: "text-risk-low",
  medium: "text-risk-medium",
  high: "text-risk-high",
  blocked: "text-risk-high",
};

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

  const ctx = approval.context;
  const candidateRows: Array<[string, string | null | undefined]> = [
    ["Requested by", ctx?.source],
    ["Matched policy", ctx?.risk_details?.matched_policy],
    ["Reason", ctx?.reason],
    ["Change ID", ctx?.change_id],
  ];
  const contextRows = candidateRows.filter((row): row is [string, string] => Boolean(row[1]));

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
          {t("Requested at")} {formatDateTime(approval.requested_at)}
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

      {contextRows.length > 0 && (
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 rounded-xl border border-line px-4 py-3 text-xs">
          {contextRows.map(([label, value]) => (
            <div key={label} className="contents">
              <dt className="text-faint">{t(label)}</dt>
              <dd className="min-w-0 break-words font-mono text-foreground">{value}</dd>
            </div>
          ))}
        </dl>
      )}

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
