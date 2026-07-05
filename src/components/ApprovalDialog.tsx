import { ShieldAlert, X } from "lucide-react";
import { useI18n } from "../i18n";
import type { RiskLevel } from "../types";
import RiskBadge from "./RiskBadge";
import { Button } from "./ui/button";
import { Dialog } from "./ui/dialog";
import { IconButton } from "./ui/icon-button";

type Props = {
  command: string;
  riskLevel: RiskLevel;
  onConfirm: () => void;
  onCancel: () => void;
};

export default function ApprovalDialog({ command, riskLevel, onConfirm, onCancel }: Props) {
  const { t } = useI18n();
  return (
    <Dialog onClose={onCancel}>
      <div className="mb-4 flex items-center gap-2.5 text-destructive">
        <ShieldAlert size={20} />
        <h3 className="flex-1 text-lg font-semibold">{t("High-risk command")}</h3>
        <IconButton onClick={onCancel}>
          <X size={16} />
        </IconButton>
      </div>

      <div className="mb-5 space-y-3">
        <p className="text-sm text-muted-foreground">
          {t("This command has been classified as:")}
        </p>
        <RiskBadge level={riskLevel} />

        <div className="rounded-md bg-muted px-3.5 py-3">
          <code className="break-all font-mono text-sm text-foreground">{command}</code>
        </div>

        <p className="text-sm text-muted-foreground">
          {t("High-risk commands may modify system state, delete data, or affect production services. Please review carefully before proceeding.")}
        </p>
      </div>

      <div className="flex justify-end gap-2.5">
        <Button id="approval-dialog-cancel" variant="secondary" onClick={onCancel}>
          {t("Cancel")}
        </Button>
        <Button variant="destructive" onClick={onConfirm}>
          <ShieldAlert size={14} />
          {t("Confirm and execute")}
        </Button>
      </div>
    </Dialog>
  );
}
