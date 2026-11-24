import { ChevronDown, ChevronRight, Search, ShieldCheck, X, XCircle } from "lucide-react";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, reportError } from "../../api";
import { useAgentEvents } from "../../eventsBus";
import { useI18n } from "../../i18n";
import type { ApprovalRequest, ApprovalStatus } from "../../types";
import ApprovalDialog from "../ApprovalDialog";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { confirmDialog } from "../ui/dialog";
import { Input } from "../ui/input";
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
  /** Focus a specific pending approval (e.g. chosen from the ⌘K palette). */
  focusRequest?: { id: string; nonce: number } | null;
  /** `command` approves that text instead of the requested one. */
  onApprove: (approval: ApprovalRequest, command?: string) => void | Promise<void>;
  onReject: (approval: ApprovalRequest) => void | Promise<void>;
  /** Open Execution with this approval's host and command. `command` is the
   *  detail draft when the operator edited one, otherwise the requested text.
   *  The copy-to-clipboard half happens in `ApprovalDetail` before this fires. */
  onGoExecute: (approval: ApprovalRequest, command?: string) => void;
};

const STATUS_BADGE: Record<Exclude<ApprovalStatus, "pending">, "success" | "destructive" | "secondary"> = {
  approved: "success",
  rejected: "destructive",
  timed_out: "secondary",
  // Fifth variant of `ApprovalStatus`: an approved request that was withdrawn.
  // No producer today (see `src/types.ts`), so this only keeps the record
  // exhaustive — without it `STATUS_BADGE[item.status]` would be `undefined`
  // and the badge would silently lose its colour the day one appears.
  revoked: "secondary",
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
function ApprovalWorkbench({ pending, focusRequest = null, onApprove, onReject, onGoExecute }: Props) {
  const { t } = useI18n();
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [confirmTarget, setConfirmTarget] = useState<ApprovalRequest | null>(null);
  // Set when the pending confirmation is for an edited command.
  const [confirmCommand, setConfirmCommand] = useState<string | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  const [history, setHistory] = useState<ApprovalRequest[]>([]);
  const [auditToday, setAuditToday] = useState<number>(0);
  const [inFlight, setInFlight] = useState<number>(0);
  const [showDecided, setShowDecided] = useState(false);
  const [mode, setMode] = useState<string>("ask");
  const [modeBusy, setModeBusy] = useState(false);
  const [queueFilter, setQueueFilter] = useState("");
  const [nowMs, setNowMs] = useState(() => Date.now());
  const detailRef = useRef<HTMLDivElement>(null);

  const visiblePending = useMemo(() => {
    const needle = queueFilter.trim().toLowerCase();
    if (!needle) return pending;
    return pending.filter((item) =>
      [item.host, item.command, item.risk_level, item.context?.source]
        .filter(Boolean)
        .some((part) => part!.toLowerCase().includes(needle)),
    );
  }, [pending, queueFilter]);

  const selected =
    visiblePending[Math.min(selectedIndex, Math.max(0, visiblePending.length - 1))] ?? null;

  // Tick once per second only while pending approvals exist so TTL countdowns stay live.
  useEffect(() => {
    if (pending.length === 0) return;
    setNowMs(Date.now());
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [pending.length]);

  // Keep the selection inside bounds as the queue shrinks/grows or filters change.
  useEffect(() => {
    setSelectedIndex((index) => Math.max(0, Math.min(index, visiblePending.length - 1)));
  }, [visiblePending.length]);

  useEffect(() => {
    if (!focusRequest?.id) return;
    setQueueFilter("");
    const matchIndex = pending.findIndex((item) => item.id === focusRequest.id);
    const targetIndex = matchIndex >= 0 ? matchIndex : 0;
    setSelectedIndex(targetIndex);
    const timer = window.setTimeout(() => {
      document.getElementById(`approval-queue-card-${targetIndex}`)?.focus();
    }, 0);
    return () => window.clearTimeout(timer);
    // Only re-run when a new focus request arrives; pending polling must not steal focus.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focusRequest]);

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

  useEffect(() => {
    api
      .getApprovalMode()
      .then(setMode)
      .catch(() => setMode("ask"));
  }, []);

  async function changeMode(next: string) {
    if (next === mode || modeBusy) return;
    setModeBusy(true);
    const previous = mode;
    setMode(next); // optimistic
    try {
      const saved = await api.setApprovalMode(next);
      setMode(saved);
    } catch (err) {
      setMode(previous);
      reportError("approval-workbench", "set approval mode failed", err);
    } finally {
      setModeBusy(false);
    }
  }

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
    (approval: ApprovalRequest, command?: string) => {
      if (!approval) return;
      if (approval.risk_level === "high" || approval.risk_level === "blocked") {
        setConfirmCommand(command);
        setConfirmTarget(approval);
        return;
      }
      void onApprove(approval, command);
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
      if (confirmTarget || visiblePending.length === 0) return;
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setSelectedIndex((index) => Math.min(visiblePending.length - 1, index + 1));
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        setSelectedIndex((index) => Math.max(0, index - 1));
      } else if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        requestApprove(visiblePending[Math.min(selectedIndex, visiblePending.length - 1)]);
      } else if (event.key === "Enter") {
        event.preventDefault();
        detailRef.current?.focus();
      } else if (event.key === "Backspace") {
        event.preventDefault();
        requestReject(visiblePending[Math.min(selectedIndex, visiblePending.length - 1)]);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [visiblePending, selectedIndex, confirmTarget, requestApprove, requestReject]);

  async function handleConfirmApprove() {
    if (!confirmTarget) return;
    const target = confirmTarget;
    const command = confirmCommand;
    setConfirmTarget(null);
    setConfirmCommand(undefined);
    setBusy(true);
    try {
      await onApprove(target, command);
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

  async function handleRejectAllShown() {
    if (visiblePending.length === 0 || busy) return;
    const targets = [...visiblePending];
    const ok = await confirmDialog({
      title: t("Reject {count} pending approvals?", { count: targets.length }),
      confirmLabel: t("Reject"),
      danger: true,
    });
    if (!ok) return;
    setBusy(true);
    try {
      for (const item of targets) {
        await onReject(item);
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-5">
      {/* Approval posture: how agent-surface requests are decided. The desktop
          itself never prompts (a human click is its own approval) — this dial
          governs the daemon / MCP / CLI cards. */}
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-xs font-bold uppercase tracking-wide text-muted-foreground">
          {t("Approval mode")}
        </span>
        {(
          [
            ["ask", t("Ask"), t("Pop an approval card per policy or high-risk command")],
            [
              "auto",
              t("AI auto-approve"),
              t("Low/medium risk is approved by the risk engine; high risk still asks"),
            ],
            ["full", t("Full access"), t("No approval cards; blocked rules and audit still apply")],
          ] as const
        ).map(([value, label, description]) => (
          <button
            key={value}
            type="button"
            disabled={modeBusy}
            title={description}
            onClick={() => void changeMode(value)}
            className={cn(
              "rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors disabled:opacity-50",
              mode === value
                ? "border-primary bg-primary/10 text-primary"
                : "border-border text-muted-foreground hover:bg-muted hover:text-foreground"
            )}
          >
            {label}
          </button>
        ))}
        <span className="min-w-0 flex-1 truncate text-[11px] text-faint">
          {mode === "ask"
            ? t("Pop an approval card per policy or high-risk command")
            : mode === "auto"
              ? t("Low/medium risk is approved by the risk engine; high risk still asks")
              : t("No approval cards; blocked rules and audit still apply")}
        </span>
      </div>

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

      {(pending.length > 1 || queueFilter.trim().length > 0) && (
        <div className="flex flex-wrap items-center gap-2">
          <div className="relative min-w-[200px] flex-1 max-w-sm">
            <Search
              size={14}
              className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground"
            />
            <Input
              className="h-8 pl-8 pr-7"
              value={queueFilter}
              onChange={(event) => setQueueFilter(event.target.value)}
              placeholder={t("Filter approvals by host or command")}
              aria-label={t("Filter approvals by host or command")}
            />
            {queueFilter && (
              <button
                type="button"
                title={t("Clear search")}
                aria-label={t("Clear search")}
                onClick={() => setQueueFilter("")}
                className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
              >
                <X size={13} />
              </button>
            )}
          </div>
          {visiblePending.length > 1 && (
            <Button
              variant="destructive"
              size="sm"
              className="ml-auto"
              disabled={busy}
              onClick={() => void handleRejectAllShown()}
            >
              <XCircle size={14} />
              {queueFilter.trim()
                ? t("Reject shown ({count})", { count: visiblePending.length })
                : t("Reject all ({count})", { count: visiblePending.length })}
            </Button>
          )}
        </div>
      )}

      <div className="grid items-start gap-4 lg:grid-cols-[250px_minmax(0,1fr)] max-lg:grid-cols-1">
        <ApprovalQueue
          items={visiblePending}
          selectedIndex={selectedIndex}
          onSelect={setSelectedIndex}
          nowMs={nowMs}
          emptyMessage={
            pending.length > 0 ? t("No pending approvals match this filter") : undefined
          }
        />
        <div ref={detailRef} tabIndex={-1} className="min-w-0 focus-visible:outline-none">
          <ApprovalDetail
            approval={selected}
            busy={busy}
            onApprove={(command) => selected && requestApprove(selected, command)}
            onReject={handleReject}
            onCopyAndExecute={(command) => selected && onGoExecute(selected, command)}
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
          command={confirmCommand ?? confirmTarget.command}
          riskLevel={confirmTarget.risk_level}
          onConfirm={handleConfirmApprove}
          onCancel={() => {
            setConfirmTarget(null);
            setConfirmCommand(undefined);
          }}
        />
      )}
    </div>
  );
}

export default memo(ApprovalWorkbench);
