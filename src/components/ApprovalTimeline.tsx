import { CheckCircle2, Loader2, RefreshCw, ShieldCheck, XCircle } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, reportError } from "../api";
import { useAgentEvents } from "../eventsBus";
import { useI18n } from "../i18n";
import type { ApprovalRequest, ApprovalStatus } from "../types";
import RiskBadge from "./RiskBadge";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { IconButton } from "./ui/icon-button";
import { EmptyState } from "./ui/state";
import { cn } from "../lib/utils";

// J3-style render cap so a long-running daemon's approval history (never
// pruned server-side) never mounts thousands of timeline rows at once.
const RENDER_CAP_STEP = 200;
const POLL_MS = 10000;

const STATUS_BADGE: Record<ApprovalStatus, { variant: "warning" | "success" | "destructive" | "secondary"; dot: string }> = {
  pending: { variant: "warning", dot: "bg-warning" },
  approved: { variant: "success", dot: "bg-success" },
  rejected: { variant: "destructive", dot: "bg-destructive" },
  timed_out: { variant: "secondary", dot: "bg-muted-foreground" },
};

function formatTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

/** V2-2: full approval history as a vertical timeline, plus batch approve/reject on pending items. */
export default function ApprovalTimeline() {
  const { t } = useI18n();
  const [approvals, setApprovals] = useState<ApprovalRequest[]>([]);
  const [loading, setLoading] = useState(true);
  const [renderCap, setRenderCap] = useState(RENDER_CAP_STEP);
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [batchBusy, setBatchBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const list = await api.fetchApprovals();
      setApprovals(list);
    } catch (err) {
      reportError("approval-timeline", "list approvals failed", err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, POLL_MS);
    return () => clearInterval(id);
  }, [refresh]);

  // Real-time nudge: refetch as soon as an approval is created or resolved,
  // instead of waiting up to POLL_MS for the next scheduled poll.
  useAgentEvents((event) => {
    if (event.event_type === "approval_requested" || event.event_type === "approval_responded") {
      refresh();
    }
  });

  const sorted = useMemo(
    () =>
      [...approvals].sort(
        (a, b) => new Date(b.requested_at).getTime() - new Date(a.requested_at).getTime()
      ),
    [approvals]
  );

  const pendingIds = useMemo(
    () => sorted.filter((a) => a.status === "pending").map((a) => a.id),
    [sorted]
  );

  const allPendingSelected = pendingIds.length > 0 && pendingIds.every((id) => selected.has(id));

  function toggleSelected(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function toggleSelectAllPending() {
    setSelected(allPendingSelected ? new Set() : new Set(pendingIds));
  }

  async function respond(id: string, approved: boolean) {
    try {
      await (approved ? api.approvalApprove(id) : api.approvalReject(id));
      await refresh();
    } catch (err) {
      reportError("approval-timeline", "respond to approval failed", err, { id, approved });
    }
    setSelected((prev) => {
      const next = new Set(prev);
      next.delete(id);
      return next;
    });
  }

  async function respondBatch(approved: boolean) {
    const ids = [...selected].filter((id) => pendingIds.includes(id));
    if (ids.length === 0) return;
    setBatchBusy(true);
    try {
      const results = await Promise.allSettled(
        ids.map((id) => (approved ? api.approvalApprove(id) : api.approvalReject(id)))
      );
      const failed = results.filter((r) => r.status === "rejected").length;
      if (failed > 0) {
        reportError(
          "approval-timeline",
          "batch respond partially failed",
          new Error(`${failed}/${ids.length} failed`),
          { approved }
        );
      }
      setSelected(new Set());
      await refresh();
    } finally {
      setBatchBusy(false);
    }
  }

  return (
    <Card className="space-y-3 p-4">
      <div className="flex items-center gap-2 font-semibold">
        <ShieldCheck size={16} className="text-muted-foreground" />
        {t("Approvals")}
        <Badge variant={pendingIds.length > 0 ? "warning" : "secondary"} className="ml-1 font-medium">
          {t("{count} pending", { count: pendingIds.length })}
        </Badge>
        <IconButton className="ml-auto" onClick={refresh} title={t("Refresh")}>
          <RefreshCw size={15} />
        </IconButton>
      </div>

      {pendingIds.length > 0 && (
        <div className="flex flex-wrap items-center gap-2 rounded-lg border border-border bg-muted/40 px-3 py-2">
          <label className="flex cursor-pointer select-none items-center gap-1.5 text-sm font-medium">
            <input
              type="checkbox"
              className="size-4 accent-primary"
              checked={allPendingSelected}
              onChange={toggleSelectAllPending}
            />
            {t("Select all pending")}
          </label>
          <span className="text-xs text-muted-foreground">
            {t("{count} selected", { count: selected.size })}
          </span>
          <div className="ml-auto flex gap-2">
            <Button
              size="sm"
              disabled={selected.size === 0 || batchBusy}
              onClick={() => respondBatch(true)}
            >
              {batchBusy ? <Loader2 size={14} className="animate-spin" /> : <CheckCircle2 size={14} />}
              {t("Approve selected")}
            </Button>
            <Button
              size="sm"
              variant="destructive"
              disabled={selected.size === 0 || batchBusy}
              onClick={() => respondBatch(false)}
            >
              {batchBusy ? <Loader2 size={14} className="animate-spin" /> : <XCircle size={14} />}
              {t("Reject selected")}
            </Button>
          </div>
        </div>
      )}

      {!loading && sorted.length === 0 && (
        <EmptyState icon={ShieldCheck} title={t("No approval history yet")} />
      )}

      {sorted.length > 0 && (
        <div className="grid gap-3 border-l border-border pl-4">
          {sorted.slice(0, renderCap).map((approval) => {
            const badge = STATUS_BADGE[approval.status];
            const isPending = approval.status === "pending";
            return (
              <div key={approval.id} className="relative">
                <span
                  className={cn(
                    "absolute -left-[21px] top-1.5 size-2.5 rounded-full ring-4 ring-background",
                    badge.dot
                  )}
                />
                <div className="grid gap-1.5 rounded-lg border border-border bg-card p-3">
                  <div className="flex flex-wrap items-center gap-2">
                    {isPending && (
                      <input
                        type="checkbox"
                        className="size-4 accent-primary"
                        checked={selected.has(approval.id)}
                        onChange={() => toggleSelected(approval.id)}
                      />
                    )}
                    <span className="text-xs text-muted-foreground">
                      {formatTime(approval.requested_at)}
                    </span>
                    <strong className="font-semibold">{approval.host}</strong>
                    <RiskBadge level={approval.risk_level} />
                    <Badge variant={badge.variant}>{t(approval.status)}</Badge>
                    {isPending && (
                      <div className="ml-auto flex gap-1.5">
                        <Button size="sm" onClick={() => respond(approval.id, true)}>
                          {t("Approve")}
                        </Button>
                        <Button size="sm" variant="destructive" onClick={() => respond(approval.id, false)}>
                          {t("Reject")}
                        </Button>
                      </div>
                    )}
                  </div>
                  <code className="block break-all rounded bg-muted px-2 py-1.5 font-mono text-xs text-foreground">
                    {approval.command}
                  </code>
                </div>
              </div>
            );
          })}
          {sorted.length > renderCap && (
            <button
              type="button"
              onClick={() => setRenderCap((c) => c + RENDER_CAP_STEP)}
              className="text-center text-xs text-muted-foreground hover:text-foreground"
            >
              {t("Show more ({count} hidden)", { count: sorted.length - renderCap })}
            </button>
          )}
        </div>
      )}
    </Card>
  );
}
