import { Search, type LucideIcon } from "lucide-react";
import { type KeyboardEvent, useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { cn } from "../lib/utils";
import type { ApprovalRequest } from "../types";
import { RISK_TEXT_CLS } from "./RiskBadge";
import { Input } from "./ui/input";

export type CommandPaletteModule = {
  id: string;
  label: string;
  icon: LucideIcon;
};

/** A one-click action rendered in the "Quick actions" group. Built by the App
 *  layer (labels already i18n-resolved there); an empty array hides the group. */
export type PaletteQuickAction = {
  id: string;
  label: string;
  icon: LucideIcon;
  run: () => void;
};

/** Frontend mirror of the daemon's ssh_* MCP tool names (see
 *  src-tauri/src/bin/agent2ssh_mcp/tools.rs — keep in sync manually). The
 *  palette cannot invoke tools (they need arguments); activating one jumps to
 *  the module that exercises the same capability. */
const MCP_TOOLS: { name: string; moduleId: string }[] = [
  { name: "ssh_list_hosts", moduleId: "hosts" },
  { name: "ssh_add_host", moduleId: "hosts" },
  { name: "ssh_remove_host", moduleId: "hosts" },
  { name: "ssh_import_config", moduleId: "hosts" },
  { name: "ssh_connect", moduleId: "hosts" },
  { name: "ssh_disconnect", moduleId: "hosts" },
  { name: "ssh_connection_status", moduleId: "hosts" },
  { name: "ssh_ping", moduleId: "hosts" },
  { name: "ssh_exec", moduleId: "execute" },
  { name: "ssh_exec_multi", moduleId: "execute" },
  { name: "ssh_exec_compare", moduleId: "execute" },
  { name: "ssh_preview_exec", moduleId: "execute" },
  { name: "ssh_risk_check", moduleId: "execute" },
  { name: "ssh_snippet_list", moduleId: "execute" },
  { name: "ssh_snippet_save", moduleId: "execute" },
  { name: "ssh_snippet_delete", moduleId: "execute" },
  { name: "ssh_audit", moduleId: "audit" },
  { name: "ssh_audit_export", moduleId: "audit" },
  { name: "ssh_metrics", moduleId: "audit" },
  { name: "ssh_metrics_trend", moduleId: "audit" },
  { name: "ssh_sftp_ls", moduleId: "files-sessions" },
  { name: "ssh_sftp_stat", moduleId: "files-sessions" },
  { name: "ssh_sftp_mkdir", moduleId: "files-sessions" },
  { name: "ssh_sftp_upload", moduleId: "files-sessions" },
  { name: "ssh_sftp_download", moduleId: "files-sessions" },
  { name: "ssh_session_open", moduleId: "terminal" },
  { name: "ssh_session_write", moduleId: "terminal" },
  { name: "ssh_session_read", moduleId: "terminal" },
  { name: "ssh_session_close", moduleId: "terminal" },
  { name: "ssh_session_list", moduleId: "terminal" },
  { name: "ssh_forward_add", moduleId: "tunnels" },
  { name: "ssh_forward_list", moduleId: "tunnels" },
  { name: "ssh_forward_remove", moduleId: "tunnels" },
  { name: "ssh_gate_status", moduleId: "dashboard" },
  { name: "ssh_health_snapshot", moduleId: "dashboard" },
  { name: "ssh_approval_list", moduleId: "approvals" },
  { name: "ssh_approval_respond", moduleId: "approvals" },
  { name: "ssh_approval_check", moduleId: "approvals" },
  { name: "ssh_approval_policies_list", moduleId: "approvals" },
  { name: "ssh_playbook_list", moduleId: "playbooks" },
  { name: "ssh_playbook_run", moduleId: "playbooks" },
  { name: "ssh_playbook_dry_run", moduleId: "playbooks" },
  { name: "ssh_config_export", moduleId: "sync" },
  { name: "ssh_config_import", moduleId: "sync" },
  { name: "ssh_config_import_preview", moduleId: "sync" },
  { name: "ssh_sync_diff", moduleId: "sync" },
  { name: "ssh_sync_export", moduleId: "sync" },
  { name: "ssh_list_daemons", moduleId: "mcp-agents" },
  { name: "ssh_daemons_view", moduleId: "mcp-agents" },
  { name: "ssh_daemon_diagnose", moduleId: "mcp-agents" },
  { name: "ssh_daemon_version_check", moduleId: "mcp-agents" },
  { name: "ssh_doctor", moduleId: "mcp-agents" },
  { name: "ssh_events_subscribe", moduleId: "mcp-agents" },
  { name: "ssh_webhook_config", moduleId: "mcp-agents" },
];

type PaletteItem =
  | { kind: "approval"; approval: ApprovalRequest }
  | { kind: "module"; id: string; label: string; icon: LucideIcon }
  | { kind: "action"; action: PaletteQuickAction }
  | { kind: "mcp-tool"; name: string; moduleId: string }
  | { kind: "host"; name: string; subtitle: string };

type PaletteSection = {
  id: "approvals" | "modules" | "actions" | "mcp" | "hosts";
  label: string;
  items: PaletteItem[];
  /** Offset of items[0] within the flat keyboard-navigation list. */
  startIndex: number;
};

type CommandPaletteProps = {
  open: boolean;
  onClose: () => void;
  modules: readonly CommandPaletteModule[];
  hosts: HostProfileLike[];
  onNavigateModule: (id: string) => void;
  onSelectHost: (name: string) => void;
  /** v3 §3.5: pending approvals pinned on top (only when N > 0). */
  pending?: ApprovalRequest[];
  /** Activated from the pending group: jump to the workbench, focus first card. */
  onOpenApprovals?: () => void;
  /** v3 §3.5 quick actions (sync / unlock secrets / new host). */
  quickActions?: PaletteQuickAction[];
};

/** Minimal structural type so the palette stays decoupled from the full HostProfile. */
type HostProfileLike = {
  name: string;
  host: string;
  user?: string | null;
  group: string;
  role?: string | null;
  owner?: string | null;
  tags?: string[] | null;
};

// v3 §3.5: pin at most this many pending rows in the palette; the count in the
// section header always shows the real queue depth.
const PENDING_RENDER_CAP = 5;
const MAX_RESULTS = 30;

/** v3 §3.5 ⌘K palette: five groups — pending approvals (top, N>0 only) →
 *  navigate → quick actions → MCP tools (`>` prefix narrows to this group) →
 *  connect host (last). Floating layer on bg-popover (canvas-3), 460px, no
 *  shadow (v3 §4.5 layer discipline). */
export default function CommandPalette({
  open,
  onClose,
  modules,
  hosts,
  onNavigateModule,
  onSelectHost,
  pending = [],
  onOpenApprovals,
  quickActions = [],
}: CommandPaletteProps) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setActiveIndex(0);
    const id = window.setTimeout(() => inputRef.current?.focus(), 0);
    return () => window.clearTimeout(id);
  }, [open]);

  const sections = useMemo<PaletteSection[]>(() => {
    const raw = query.trim();
    // ">" prefix narrows the palette to MCP tools only (design §3.5).
    const mcpOnly = raw.startsWith(">");
    const q = (mcpOnly ? raw.slice(1) : raw).trim().toLowerCase();

    const approvalItems: PaletteItem[] =
      !mcpOnly && onOpenApprovals && pending.length > 0
        ? pending
            .slice(0, PENDING_RENDER_CAP)
            .map((approval) => ({ kind: "approval" as const, approval }))
        : [];

    const moduleItems: PaletteItem[] = mcpOnly
      ? []
      : modules
          .filter(
            (m) => !q || t(m.label).toLowerCase().includes(q) || m.id.includes(q)
          )
          .map((m) => ({ kind: "module" as const, id: m.id, label: m.label, icon: m.icon }));

    const actionItems: PaletteItem[] =
      !mcpOnly && quickActions.length > 0
        ? quickActions
            .filter((a) => !q || a.label.toLowerCase().includes(q))
            .map((action) => ({ kind: "action" as const, action }))
        : [];

    const mcpItems: PaletteItem[] = MCP_TOOLS.filter(
      (tool) => !q || tool.name.includes(q)
    ).map((tool) => ({ kind: "mcp-tool" as const, name: tool.name, moduleId: tool.moduleId }));

    const hostItems: PaletteItem[] = mcpOnly
      ? []
      : hosts
          .filter((h) => {
            if (!q) return true;
            const haystack = [
              h.name,
              h.host,
              h.user ?? "",
              h.group,
              h.role ?? "",
              h.owner ?? "",
              ...(h.tags ?? []),
            ]
              .join(" ")
              .toLowerCase();
            return haystack.includes(q);
          })
          .map((h) => ({
            kind: "host" as const,
            name: h.name,
            subtitle: `${h.user ? `${h.user}@` : ""}${h.host}${h.tags?.length ? ` · ${h.tags.join(", ")}` : ""}`,
          }));

    const built: PaletteSection[] = [
      { id: "approvals", label: t("Pending approvals"), items: approvalItems, startIndex: 0 },
      { id: "modules", label: t("Navigate"), items: moduleItems, startIndex: 0 },
      { id: "actions", label: t("Quick actions"), items: actionItems, startIndex: 0 },
      { id: "mcp", label: t("MCP tools"), items: mcpItems, startIndex: 0 },
      { id: "hosts", label: t("Connect host"), items: hostItems, startIndex: 0 },
    ];
    // Drop empty sections and apply the global result cap in group order.
    const capped: PaletteSection[] = [];
    let budget = MAX_RESULTS;
    for (const section of built) {
      if (budget <= 0) break;
      const items = section.items.slice(0, budget);
      budget -= items.length;
      if (items.length === 0) continue;
      const startIndex = capped.reduce((sum, prev) => sum + prev.items.length, 0);
      capped.push({ ...section, items, startIndex });
    }
    return capped;
  }, [query, modules, hosts, pending, onOpenApprovals, quickActions, t]);

  const flatCount = useMemo(
    () => sections.reduce((sum, section) => sum + section.items.length, 0),
    [sections]
  );

  useEffect(() => {
    setActiveIndex((idx) => Math.min(idx, Math.max(flatCount - 1, 0)));
  }, [flatCount]);

  if (!open) return null;

  /** Flat keyboard index of an item inside its section. */
  function flatIndex(section: PaletteSection, item: PaletteItem): number {
    return section.startIndex + section.items.indexOf(item);
  }

  function activate(item: PaletteItem) {
    switch (item.kind) {
      case "approval":
        // ↵ on a pending approval: workbench + focus the first card (plan §1.3).
        onOpenApprovals?.();
        break;
      case "module":
        onNavigateModule(item.id);
        break;
      case "action":
        item.action.run();
        break;
      case "mcp-tool":
        onNavigateModule(item.moduleId);
        break;
      case "host":
        onNavigateModule("hosts");
        onSelectHost(item.name);
        break;
    }
    onClose();
  }

  function handleKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIndex((i) => Math.min(i + 1, flatCount - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const section = sections.find(
        (s) => activeIndex >= s.startIndex && activeIndex < s.startIndex + s.items.length
      );
      const item = section?.items[activeIndex - (section?.startIndex ?? 0)];
      if (item) activate(item);
    }
  }

  return (
    <div
      className="fixed inset-0 z-[1200] flex items-start justify-center bg-black/50 p-4 pt-[12vh] max-sm:items-stretch max-sm:p-0"
      onClick={onClose}
    >
      <div
        className="w-[460px] max-w-[calc(100vw-2rem)] overflow-hidden rounded-xl border border-edge bg-popover text-popover-foreground max-sm:h-full max-sm:max-w-full max-sm:rounded-none max-sm:border-none"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2 border-b border-border px-4 py-3">
          <Search size={16} className="shrink-0 text-muted-foreground" />
          <Input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={t("Search modules, hosts, tools... (> for MCP tools)")}
            className="h-8 border-none bg-transparent px-0 shadow-none focus-visible:ring-0"
          />
          <kbd className="shrink-0 rounded border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground">
            Esc
          </kbd>
        </div>
        <div className="max-h-[50vh] overflow-y-auto py-1 max-sm:max-h-[calc(100%-53px)]">
          {flatCount === 0 && (
            <div className="px-4 py-6 text-center text-sm text-muted-foreground">
              {t("No matches")}
            </div>
          )}
          {sections.map((section) => (
            <div key={section.id} className="py-1">
              <div className="px-4 pb-1 pt-1.5 text-[10px] font-semibold uppercase tracking-wide text-faint">
                {section.label}
                {section.id === "approvals" && pending.length > 0 && (
                  <span className="ml-1.5 font-mono normal-case">{pending.length}</span>
                )}
              </div>
              {section.items.map((item) => {
                const index = flatIndex(section, item);
                const active = index === activeIndex;
                  const key =
                    item.kind === "approval"
                      ? `approval-${item.approval.id}`
                      : item.kind === "module"
                        ? `module-${item.id}`
                        : item.kind === "action"
                          ? `action-${item.action.id}`
                          : item.kind === "mcp-tool"
                            ? `mcp-${item.name}`
                            : `host-${item.name}`;
                  return (
                    <button
                      key={key}
                      type="button"
                      className={cn(
                        "flex w-full items-center gap-3 px-4 py-2 text-left text-sm",
                        active ? "bg-muted text-foreground" : "text-foreground/90 hover:bg-muted/60"
                      )}
                      onMouseEnter={() => setActiveIndex(index)}
                      onClick={() => activate(item)}
                    >
                      {item.kind === "approval" && (
                        <>
                          <span className="flex size-[15px] shrink-0 items-center justify-center rounded-sm bg-risk-medium/15 text-[9px] font-bold text-risk-medium">
                            {item.approval.risk_level.slice(0, 1).toUpperCase()}
                          </span>
                          <span className="min-w-0 flex-1 truncate">
                            <span className="font-mono font-semibold">{item.approval.host}</span>
                            <code
                              className={cn(
                                "ml-2 font-mono text-xs",
                                RISK_TEXT_CLS[item.approval.risk_level]
                              )}
                            >
                              {item.approval.command}
                            </code>
                          </span>
                          <span className="ml-auto shrink-0 font-mono text-[10px] uppercase text-muted-foreground/60">
                            ↵
                          </span>
                        </>
                      )}
                      {item.kind === "module" && (
                        <>
                          <item.icon size={15} className="shrink-0 text-muted-foreground" />
                          <span className="truncate">{t(item.label)}</span>
                          <span className="ml-auto shrink-0 text-[10px] uppercase text-muted-foreground/60">
                            {t("Module")}
                          </span>
                        </>
                      )}
                      {item.kind === "action" && (
                        <>
                          <item.action.icon size={15} className="shrink-0 text-primary" />
                          <span className="truncate">{item.action.label}</span>
                          <span className="ml-auto shrink-0 text-[10px] uppercase text-muted-foreground/60">
                            {t("Action")}
                          </span>
                        </>
                      )}
                      {item.kind === "mcp-tool" && (
                        <>
                          <span className="shrink-0 font-mono text-xs text-muted-foreground">
                            &gt;
                          </span>
                          <span className="min-w-0 flex-1 truncate font-mono text-xs">
                            {item.name}
                          </span>
                          <span className="ml-auto shrink-0 text-[10px] uppercase text-muted-foreground/60">
                            MCP
                          </span>
                        </>
                      )}
                      {item.kind === "host" && (
                        <>
                          <span className="flex size-[15px] shrink-0 items-center justify-center rounded-sm bg-muted text-[9px] font-bold text-muted-foreground">
                            {item.name.slice(0, 1).toUpperCase()}
                          </span>
                          <span className="min-w-0 flex-1 truncate">
                            {item.name}
                            <span className="ml-2 text-xs text-muted-foreground">
                              {item.subtitle}
                            </span>
                          </span>
                          <span className="ml-auto shrink-0 text-[10px] uppercase text-muted-foreground/60">
                            {t("Host")}
                          </span>
                        </>
                      )}
                    </button>
                  );
                })}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
