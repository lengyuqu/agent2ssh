import { useEffect, useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import type { ApprovalMode, HostPolicySummary } from "../types";
import { cn } from "../lib/utils";

type Props = {
  /** The host whose live policy to show; null shows the default policy. */
  host: string | null;
};

const MODE_LABEL: Record<ApprovalMode, string> = {
  auto: "auto-approve",
  required: "need approval",
  conditional: "depends on command",
};

/** v3 3.2: the policy summary line. With one host selected it shows that
 * host's live policy from the daemon; otherwise (or when the daemon is
 * unreachable) it falls back to the default policy text. */
export default function HostPolicyLine({ host }: Props) {
  const { t } = useI18n();
  const [summary, setSummary] = useState<HostPolicySummary | null>(null);

  useEffect(() => {
    setSummary(null);
    if (!host) return;
    let cancelled = false;
    api.fetchHostPolicy(host).then((result) => {
      if (!cancelled) setSummary(result);
    });
    return () => {
      cancelled = true;
    };
  }, [host]);

  const segments: Array<{ key: string; label: string; tone: string; dot: string }> = summary
    ? [
        {
          key: "low",
          label: `low ${t(MODE_LABEL[summary.low])}`,
          tone: "text-risk-low",
          dot: "bg-risk-low",
        },
        {
          key: "medium",
          label: `med ${t(MODE_LABEL[summary.medium])}`,
          tone: "text-risk-medium",
          dot: "bg-risk-medium",
        },
        {
          key: "high",
          label: `high ${t(MODE_LABEL[summary.high])}`,
          tone: "text-risk-high",
          dot: "bg-risk-high",
        },
        {
          key: "blocked",
          label:
            summary.blocked_patterns.length > 0
              ? t("deletion forbidden +{count} custom", { count: summary.blocked_patterns.length })
              : t("deletion forbidden"),
          tone: "text-risk-high",
          dot: "bg-risk-high",
        },
      ]
    : [
        { key: "low", label: t("low auto-approve"), tone: "text-risk-low", dot: "bg-risk-low" },
        {
          key: "medium",
          label: t("med/high need approval"),
          tone: "text-risk-medium",
          dot: "bg-risk-medium",
        },
        {
          key: "blocked",
          label: t("deletion forbidden"),
          tone: "text-risk-high",
          dot: "bg-risk-high",
        },
      ];

  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg border border-line bg-canvas-1 px-3 py-2 font-mono text-[11px] uppercase tracking-wide">
      {summary && (
        <span className="text-faint normal-case">{summary.host}</span>
      )}
      {segments.map((segment, index) => (
        <span key={segment.key} className="contents">
          {(index > 0 || summary) && (
            <span className="text-faint" aria-hidden>
              ·
            </span>
          )}
          <span className={cn("flex items-center gap-1.5", segment.tone)}>
            <span className={cn("size-1.5 rounded-full", segment.dot)} aria-hidden />
            {segment.label}
          </span>
        </span>
      ))}
    </div>
  );
}
