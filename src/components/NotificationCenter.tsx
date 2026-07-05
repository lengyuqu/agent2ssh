import { useRef } from "react";
import { api, reportError } from "../api";
import { useAgentEvents } from "../eventsBus";
import { useI18n } from "../i18n";
import { useToast } from "./ui/toast";

function asString(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

/**
 * V2-1: ambient, non-blocking notification layer built on the shared SSE event
 * bus. Approval requests get an actionable toast (approve/reject inline) in
 * addition to the existing blocking ApprovalDialog in App.tsx — acting on
 * either resolves the same server-side approval, so there is no risk of them
 * disagreeing, just a brief window where both are available. Anomalies are
 * read-only. Connection status-change notifications are NOT wired here: the
 * `host_connected`/`host_disconnected` event types exist in the backend enum
 * but nothing currently publishes them, so that notification is driven from
 * App.tsx's existing 5s connection-status poll instead (see pollConnections).
 */
export default function NotificationCenter() {
  const { t } = useI18n();
  const { showToast, dismissToast } = useToast();
  const approvalToastIds = useRef<Map<string, number>>(new Map());

  useAgentEvents((event) => {
    if (event.event_type === "approval_requested") {
      const id = asString(event.data.id);
      const host = asString(event.data.host) ?? "";
      const command = asString(event.data.command) ?? "";
      if (!id) return;
      const toastId = showToast(
        "warning",
        `${host}: ${command}`,
        {
          title: t("Approval requested"),
          durationMs: null,
          actions: [
            {
              label: t("Approve"),
              onClick: () => {
                api
                  .approvalApprove(id)
                  .catch((err) => {
                    showToast("error", t("Failed to approve: {error}", { error: String(err) }));
                    reportError("notification-center", "approve from toast failed", err, { id });
                  });
              },
            },
            {
              label: t("Reject"),
              onClick: () => {
                api
                  .approvalReject(id)
                  .catch((err) => {
                    showToast("error", t("Failed to reject: {error}", { error: String(err) }));
                    reportError("notification-center", "reject from toast failed", err, { id });
                  });
              },
            },
          ],
        }
      );
      approvalToastIds.current.set(id, toastId);
      return;
    }

    if (event.event_type === "approval_responded") {
      const id = asString(event.data.id);
      if (!id) return;
      const toastId = approvalToastIds.current.get(id);
      if (toastId !== undefined) {
        dismissToast(toastId);
        approvalToastIds.current.delete(id);
      }
      return;
    }

    if (event.event_type === "anomaly_detected") {
      const host = asString(event.data.host);
      const reason = asString(event.data.reason) ?? t("Anomaly detected");
      const kind = asString(event.data.kind);
      showToast("warning", host ? `${host}: ${reason}` : reason, {
        title: kind ? t("Anomaly: {kind}", { kind: kind.split("_").join(" ") }) : t("Anomaly detected"),
        durationMs: 8000,
      });
    }
  });

  return null;
}
