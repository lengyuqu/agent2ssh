import { useEffect, useState } from "react";
import { ArrowDownToLine, ArrowUpFromLine, X } from "lucide-react";
import { useI18n } from "../../i18n";
import { cn } from "../../lib/utils";
import { subscribeTransfers, type TransferSnapshot } from "./transferBus";

type Entry = { host: string; snapshot: TransferSnapshot; finishedAt: number | null };

/**
 * Workspace-level transfer strip: one row per host with a live SFTP transfer,
 * aggregated from every embedded file panel through the transfer bus. Renders
 * nothing while nothing moves; a finished row lingers briefly so completion
 * is visible before the entry disappears.
 */
export default function TransferCenter() {
  const { t } = useI18n();
  const [entries, setEntries] = useState<Entry[]>([]);
  const [dismissed, setDismissed] = useState(false);

  useEffect(
    () =>
      subscribeTransfers((host, snapshot) => {
        setEntries((current) => {
          const rest = current.filter((entry) => entry.host !== host);
          if (snapshot === null) {
            // Nothing in flight: keep a brief "finished" ghost row if there
            // was one, otherwise stay silent.
            const previous = current.find((entry) => entry.host === host);
            if (!previous || previous.finishedAt !== null) return rest;
            return [...rest, { host, snapshot: previous.snapshot, finishedAt: Date.now() }];
          }
          return [...rest, { host, snapshot, finishedAt: null }];
        });
      }),
    [],
  );

  // Ghost rows disappear after a short delay.
  useEffect(() => {
    if (!entries.some((entry) => entry.finishedAt !== null)) return;
    const id = window.setInterval(() => {
      setEntries((current) => {
        const now = Date.now();
        const next = current.filter(
          (entry) => entry.finishedAt === null || now - entry.finishedAt < 2500,
        );
        return next.length === current.length ? current : next;
      });
    }, 1000);
    return () => window.clearInterval(id);
  }, [entries]);

  const visible = entries.filter((entry) => !dismissed || entry.finishedAt === null);
  if (visible.length === 0) return null;

  return (
    <div className="shrink-0 border-t border-line bg-sidebar/60 px-2 py-1.5">
      <div className="mb-1 flex items-center gap-2">
        <span className="text-[10px] font-bold uppercase tracking-wide text-muted-foreground">
          {t("Transfer center")}
        </span>
        <button
          type="button"
          className="ml-auto rounded p-0.5 text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={() => setDismissed(true)}
          title={t("Close")}
        >
          <X size={12} />
        </button>
      </div>
      <div className="flex flex-col gap-1">
        {visible.map((entry) => {
          const { snapshot } = entry;
          const percent =
            snapshot.bytesTotal > 0
              ? Math.min(100, Math.round((snapshot.bytesDone / snapshot.bytesTotal) * 100))
              : snapshot.total > 0
                ? Math.round((snapshot.done / snapshot.total) * 100)
                : 0;
          const finished = entry.finishedAt !== null || snapshot.done >= snapshot.total;
          return (
            <div key={entry.host} className="flex items-center gap-2 text-xs">
              <span className="w-32 shrink-0 truncate font-medium text-foreground" title={entry.host}>
                {entry.host}
              </span>
              <span className="w-4 shrink-0 text-muted-foreground">
                {finished ? (
                  <ArrowDownToLine size={12} className="text-success" />
                ) : (
                  <ArrowUpFromLine size={12} />
                )}
              </span>
              <span className="min-w-0 flex-1 truncate text-muted-foreground" title={snapshot.current}>
                {finished ? t("Transfer complete") : snapshot.current}
              </span>
              <span className="w-24 shrink-0 text-right tabular-nums text-muted-foreground">
                {snapshot.done}/{snapshot.total}
              </span>
              <div className="h-1 w-28 shrink-0 overflow-hidden rounded bg-border">
                <div
                  className={cn("h-full rounded transition-all", finished ? "bg-success" : "bg-primary")}
                  style={{ width: `${finished ? 100 : percent}%` }}
                />
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
