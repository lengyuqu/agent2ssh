import { ChevronDown, ChevronRight, ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, reportError } from "../../api";
import { useAgentEvents } from "../../eventsBus";
import { useI18n } from "../../i18n";
import type { ApprovalRequest, ApprovalStatus } from "../../types";
import ApprovalDialog from "../ApprovalDialog";
import { Badge } from "../ui/badge";
import ApprovalDetail from "./ApprovalDetail";
import ApprovalQueue from "./ApprovalQueue";
import { cn } from "../../lib/utils";
import { formatClockTime } from "../../lib/format";

const HISTORY_POLL_MS = 10000;
const STATS_POLL_MS = 30000;
// Render cap for the collapsed "decided today" group (render-cap pattern
// inherited from the retired approval history timeline; full history search
// stays P1).
const DECIDED_RENDER_CAP = 50;
// Commands whose exit code was never recorded within this window count as
// "in flight" in the faint stats row (frontend-only approximation).
const IN_FLIGHT_WINDOW_MS = 15 * 60 * 1000;

type Props = {
  pending: ApprovalRequest[];
  onApprove: (approval: ApprovalRequest) => void | Promise<void>;
  onReject: (approval: ApprovalRequest) => void | Promise<void>;
  /** Plan §5 risk #4: "edit then execute" degrades to copy + jump to Execution. */
  onGoExecute: (approval: ApprovalRequest) => void;
};

const STATUS_BADGE: Record<Exclude<ApprovalStatus, "pending">, "success" | "destructive" | "secondary"> = {
  approved: "success",
  rejected: "destructive",
  timed_out: "secondary",
};

function isSameLocalDay(iso: string): boolean {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return false;
  const now = new Date();
  return (
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate()
  );
}

/** v3 3.1 approval workbench: big "N awaiting your decision" header, 250px
 * pending queue + detail column, and a collapsed "decided today" group.
 * Owns its own 10s history poll + SSE nudge (pattern inherited from the
 * retired approval history timeline); the pending list itself streams from App. */
export default function ApprovalWorkbench({ pending, onApprove, onReject, onGoExecute }: Props) {
  const { t } = useI18n();
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [confirmTarget, setConfirmTarget] = useState<ApprovalRequest | null>(null);
  const [busy, setBusy] = useState(false);
  const [history, setHistory] = useState<ApprovalRequest[]>([]);
  const [auditToday, setAuditToday] = useState<number>(0);
  const [inFlight, setInFlight] = useState<number>(0);
  const [showDecided, setShowDecided] = useState(false);
  const detailRef = useRef<HTMLDivElement>(null);

  const selected = pending[Math.min(selectedIndex, Math.max(0, pending.length - 1))] ?? null;

  // Keep the selection inside bounds as the queue shrinks/grows.
  useEffect(() => {
    setSelectedIndex((index) => Math.max(0, Math.min(index, pending.length - 1)));
  }, [pending.length]);

  const refreshHistory = useCallback(async () => {
    try {
      setHistory(await api.fetchApprovals());
    } catch (err) {
      reportError("approval-workbench", "list approvals failed", err);
    }
  }, []);

  const refreshStats = useCallback(async () => {
    try {
      const list = await api.listAudit({ limit: 500 });
      const now = Date.now();
      setAuditToday(list.filter((entry) => isSameLocalDay(entry.ts)).length);
      setInFlight(
        list.filter(
          (entry) =>
            entry.exit_code === null &&
            now - new Date(entry.ts).getTime() <= IN_FLIGHT_WINDOW_MS
        ).length
      );
    } catch (err) {
      reportError("approval-workbench", "load audit stats failed", err);
    }
  }, []);

  useEffect(() => {
    void refreshHistory();
    void refreshStats();
    const historyId = window.setInterval(() => void refreshHistory(), HISTORY_POLL_MS);
    const statsId = window.setInterval(() => void refreshStats(), STATS_POLL_MS);
    return () => {
      window.clearInterval(historyId);
      window.clearInterval(statsId);
    };
  }, [refreshHistory, refreshStats]);

  // SSE nudge: refresh history/stats as soon as an approval or exec lands.
  useAgentEvents((event) => {
    if (
      event.event_type === "approval_requested" ||
      event.event_type === "approval_responded" ||
      event.event_type === "exec_completed"
    ) {
      void refreshHistory();
      void refreshStats();
    }
  });

  const decidedToday = useMemo(
    () =>
      history
        .filter((item) => item.status !== "pending" && isSameLocalDay(item.requested_at))
        .sort((a, b) => new Date(b.requested_at).getTime() - new Date(a.requested_at).getTime()),
    [history]
  );

  /** high/blocked approvals must pass the two-step confirm dialog first. */
  const requestApprove = useCallback(
    (approval: ApprovalRequest) => {
      if (!approval) return;
      if (approval.risk_level === "high" || approval.risk_level === "blocked") {
        setConfirmTarget(approval);
        return;
      }
      void onApprove(approval);
    },
    [onApprove]
  );

  const requestReject = useCallback(
    (approval: ApprovalRequest) => {
      if (!approval) return;
      void onReject(approval);
    },
    [onReject]
  );

  // Keyboard: ↑↓ select, ↵ review (focus detail), ⌘↵ approve, ⌫ reject.
  // Ignored while typing in inputs; Backspace intentionally never confirms.
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      const target = event.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "SELECT" ||
          target.isContentEditable)
      ) {
        return;
      }
      if (confirmTarget || pending.length === 0) return;
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setSelectedIndex((index) => Math.min(pending.length - 1, index + 1));
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        setSelectedIndex((index) => Math.max(0, index - 1));
      } else if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        requestApprove(pending[Math.min(selectedIndex, pending.length - 1)]);
      } else if (event.key === "Enter") {
        event.preventDefault();
        detailRef.current?.focus();
      } else if (event.key === "Backspace") {
        event.preventDefault();
        requestReject(pending[Math.min(selectedIndex, pending.length - 1)]);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [pending, selectedIndex, confirmTarget, requestApprove, requestReject]);

  async function handleConfirmApprove() {
    if (!confirmTarget) return;
    const target = confirmTarget;
    setConfirmTarget(null);
    setBusy(true);
    try {
      await onApprove(target);
    } finally {
      setBusy(false);
    }
  }

  async function handleReject() {
    if (!selected) return;
    setBusy(true);
    try {
      await onReject(selected);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-5">
      {/* Opening header: 30px headline number + faint stats */}
      <div>
        <div className="flex items-end gap-3">
          <span className="text-[30px] font-bold leading-none tabular-nums">{pending.length}</span>
          <span className="pb-0.5 text-sm text-muted-foreground">
            {t("items awaiting your decision")}
          </span>
        </div>
        <div className="mt-1.5 flex gap-4 font-mono text-[11px] text-faint">
          <span>
            {t("In flight")} {inFlight}
          </span>
          <span>
            {t("Decided today")} {auditToday}
          </span>
        </div>
      </div>

      <div className="grid items-start gap-4 lg:grid-cols-[250px_minmax(0,1fr)] max-lg:grid-cols-1">
        <ApprovalQueue items={pending} selectedIndex={selectedIndex} onSelect={setSelectedIndex} />
        <div ref={detailRef} tabIndex={-1} className="min-w-0 focus-visible:outline-none">
          <ApprovalDetail
            approval={selected}
            busy={busy}
            onApprove={() => requestApprove(selected)}
            onReject={handleReject}
            onCopyAndExecute={() => selected && onGoExecute(selected)}
          />
        </div>
      </div>

      {/* Plan §5 risk #7: today's resolved approvals live in a collapsed group
          here instead of a separate history screen. */}
      {decidedToday.length > 0 && (
        <div className="rounded-xl border border-line bg-canvas-2">
          <button
            type="button"
            className="flex w-full items-center gap-2 px-4 py-2.5 text-left text-sm font-semibold"
            onClick={() => setShowDecided((value) => !value)}
            aria-expanded={showDecided}
          >
            {showDecided ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
            <ShieldCheck size={15} className="text-muted-foreground" />
            {t("Decided today")}
            <Badge variant="secondary" className="ml-1 font-mono">
              {decidedToday.length}
            </Badge>
          </button>
          {showDecided && (
            <div className="grid gap-1.5 border-t border-line px-4 py-3">
              {decidedToday.slice(0, DECIDED_RENDER_CAP).map((item) => (
                <div key={item.id} className="flex items-center gap-2 text-xs">
                  <span className="font-mono text-[11px] text-faint">
                    {formatClockTime(item.requested_at)}
                  </span>
                  <span className="truncate font-mono font-semibold">{item.host}</span>
                  <code className="min-w-0 flex-1 truncate font-mono text-muted-foreground">
                    {item.command}
                  </code>
                  <Badge variant={STATUS_BADGE[item.status as Exclude<ApprovalStatus, "pending">]}>
                    {t(item.status)}
                  </Badge>
                </div>
              ))}
              {decidedToday.length > DECIDED_RENDER_CAP && (
                <p className={cn("text-center text-[11px] text-faint")}>
                  {t("Show more ({count} hidden)", {
                    count: decidedToday.length - DECIDED_RENDER_CAP,
                  })}
                </p>
              )}
            </div>
          )}
        </div>
      )}

      {/* High/blocked two-step confirmation (⌘↵ path included) */}
      {confirmTarget && (
        <ApprovalDialog
          command={confirmTarget.command}
          riskLevel={confirmTarget.risk_level}
          onConfirm={handleConfirmApprove}
          onCancel={() => setConfirmTarget(null)}
        />
      )}
    </div>
  );
}
