import { Clipboard } from "lucide-react";
import { useI18n } from "../i18n";
import type { AuditDecision, AuditEntry, RiskLevel } from "../types";
import { formatDateTime } from "../lib/format";
import { cn } from "../lib/utils";
import RiskBadge from "./RiskBadge";
import { Button } from "./ui/button";
import { SlideOverDrawer } from "./ui/slide-over-drawer";
import { useToast } from "./ui/toast";

const DECISION_LABEL: Record<AuditDecision, string> = {
  approved: "Approved",
  approved_rewritten: "Approved (edited)",
  rejected: "Rejected",
  timed_out: "Timed out",
  auto_allowed: "Auto-allowed",
  blocked: "Blocked",
};

const COMMAND_TINT: Record<RiskLevel, string> = {
  low: "text-risk-low",
  medium: "text-risk-medium",
  high: "text-risk-high",
  blocked: "text-risk-high",
};

type Props = {
  entry: AuditEntry | null;
  onClose: () => void;
  onReplay?: (entry: AuditEntry) => void;
  onEditAndExecute?: (entry: AuditEntry) => void;
};

/** Right-side audit detail. The shell is the shared slide-over. */
export default function AuditDetailDrawer({ entry, onClose, onReplay, onEditAndExecute }: Props) {
  const { t } = useI18n();
  const { showToast } = useToast();

  if (!entry) return null;

  const original = entry.original_command?.trim() ? entry.original_command : null;

  function copyCommand(command: string) {
    const clipboard = navigator.clipboard;
    if (!clipboard?.writeText) {
      showToast("error", t("Clipboard is not available in this context."));
      return;
    }
    clipboard
      .writeText(command)
      .then(() => showToast("success", t("Copied command")))
      .catch((err) => showToast("error", String(err)));
  }

  const rows: Array<[string, string]> = [
    ["time", formatDateTime(entry.ts) || t("Not recorded")],
    ["host", entry.host],
    ["Duration", `${entry.duration_ms} ms`],
    ["Exit code", entry.exit_code === null || entry.exit_code === undefined ? t("Not recorded") : String(entry.exit_code)],
    ["Decided by", entry.decided_by?.trim() || t("Not recorded")],
    ["Request source", entry.source?.trim() || t("Not recorded")],
    ["Reason", entry.reason?.trim() || t("Not recorded")],
    ["Change ID", entry.change_id?.trim() || t("Not recorded")],
  ];

  return (
    <SlideOverDrawer
      open
      title={t("Audit detail")}
      onClose={onClose}
      widthClassName="w-[520px]"
      overlayTestId="audit-detail-overlay"
      titleId="audit-detail-title"
    >
      <div className="space-y-4" data-testid="audit-detail-drawer">
          {original ? (
            <div className="grid gap-3">
              <CommandBlock label={t("Original request")} command={original} />
              <CommandBlock
                label={t("Executed command")}
                command={entry.command}
                tint={COMMAND_TINT[entry.risk_level]}
                onCopy={() => copyCommand(entry.command)}
                copyLabel={t("Copy command")}
              />
            </div>
          ) : (
            <CommandBlock
              label={t("Command")}
              command={entry.command}
              tint={COMMAND_TINT[entry.risk_level]}
              onCopy={() => copyCommand(entry.command)}
              copyLabel={t("Copy command")}
            />
          )}

          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 rounded-xl border border-line px-4 py-3 text-xs">
            {rows.map(([label, value]) => (
              <div key={label} className="contents">
                <dt className="text-faint">{t(label)}</dt>
                <dd className="min-w-0 break-words font-mono text-foreground">{value}</dd>
              </div>
            ))}
            <div className="contents">
              <dt className="text-faint">{t("Risk level")}</dt>
              <dd>
                <RiskBadge level={entry.risk_level} />
              </dd>
            </div>
            <div className="contents">
              <dt className="text-faint">{t("Decision")}</dt>
              <dd className="min-w-0 break-words text-foreground">
                {entry.decision ? t(DECISION_LABEL[entry.decision]) : t("Not recorded")}
              </dd>
            </div>
          </dl>
        {(onReplay || onEditAndExecute) && (
          <div className="flex flex-wrap gap-2 border-t border-border pt-4">
            {onReplay && (
              <Button variant="secondary" onClick={() => onReplay(entry)}>
                {t("Replay in terminal")}
              </Button>
            )}
            {onEditAndExecute && (
              <Button onClick={() => onEditAndExecute(entry)}>{t("Edit in Execution")}</Button>
            )}
          </div>
        )}
      </div>
    </SlideOverDrawer>
  );
}

function CommandBlock({
  label,
  command,
  tint,
  onCopy,
  copyLabel,
}: {
  label: string;
  command: string;
  tint?: string;
  onCopy?: () => void;
  copyLabel?: string;
}) {
  return (
    <div className="grid gap-1.5">
      <div className="flex items-center gap-2">
        <span className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{label}</span>
        {onCopy && copyLabel && (
          <Button variant="secondary" size="sm" className="ml-auto" onClick={onCopy}>
            <Clipboard size={13} />
            {copyLabel}
          </Button>
        )}
      </div>
      <pre
        className={cn(
          "m-0 whitespace-pre-wrap break-all rounded-xl border border-line bg-canvas-0 p-3 font-mono text-[13px] leading-relaxed",
          tint,
        )}
      >
        {command}
      </pre>
    </div>
  );
}
