import { ShieldAlert, X } from "lucide-react";
import type { RiskLevel } from "../types";

type Props = {
  command: string;
  riskLevel: RiskLevel;
  onConfirm: () => void;
  onCancel: () => void;
};

export default function ApprovalDialog({
  command,
  riskLevel,
  onConfirm,
  onCancel,
}: Props) {
  return (
    <div className="approval-overlay" onClick={onCancel}>
      <div className="approval-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="approval-header">
          <ShieldAlert size={20} />
          <h3>High-risk command</h3>
          <button className="icon-button" onClick={onCancel}>
            <X size={16} />
          </button>
        </div>

        <div className="approval-body">
          <p>This command has been classified as:</p>
          <span className={`risk-badge risk-${riskLevel}`}>{riskLevel}</span>

          <div className="approval-command">
            <code>{command}</code>
          </div>

          <p>
            High-risk commands may modify system state, delete data, or affect
            production services. Please review carefully before proceeding.
          </p>
        </div>

        <div className="approval-actions">
          <button className="secondary" onClick={onCancel}>
            Cancel
          </button>
          <button className="primary approval-confirm" onClick={onConfirm}>
            <ShieldAlert size={14} />
            Confirm and execute
          </button>
        </div>
      </div>
    </div>
  );
}
