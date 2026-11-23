import {
  Activity,
  ArrowLeftRight,
  BookOpen,
  Bot,
  Boxes,
  Camera,
  Clapperboard,
  Cloud,
  FolderOpen,
  HelpCircle,
  History,
  Key,
  LayoutDashboard,
  Network,
  Play,
  Server,
  ShieldCheck,
  Terminal,
  Timer,
  Waypoints,
} from "lucide-react";

/**
 * The desktop module registry — the single home for what a module is called,
 * which icon it wears, where it sits in the sidebar, and what it links to.
 *
 * These lists have to move together, and one missing entry is invisible until
 * a runtime click fails: `resident-sessions` was added to `MODULES` but not to
 * `SIDEBAR_ORDER`, which left a finished panel with no way to reach it. Every
 * invariant that ties them together is pinned in `nav.test.ts` instead.
 *
 * `MODULES` order carries no meaning for the user: the sidebar renders from
 * `SIDEBAR_GROUPS`. Ctrl/⌘+1..9 indexes `SHORTCUT_MODULES`, the daily set,
 * because cockpit grouping puts Hosts past the ninth row.
 */
export const MODULES = [
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "hosts", label: "Host Management", icon: Server },
  { id: "proxies", label: "Proxies", icon: Network },
  { id: "terminal", label: "Terminal", icon: Terminal },
  { id: "execute", label: "Execution", icon: Play },
  { id: "files-sessions", label: "Files", icon: FolderOpen },
  { id: "tunnels", label: "Tunnels", icon: ArrowLeftRight },
  { id: "activity", label: "Activity", icon: Activity },
  { id: "mcp-agents", label: "MCP/CLI", icon: Bot },
  { id: "sync", label: "Sync", icon: Cloud },
  { id: "keys", label: "Keys", icon: Key },
  { id: "playbooks", label: "Playbooks", icon: BookOpen },
  { id: "audit", label: "Audit", icon: History },
  { id: "help", label: "Help", icon: HelpCircle },
  { id: "approvals", label: "Approvals", icon: ShieldCheck },
  { id: "config", label: "Config Snapshots", icon: Camera },
  { id: "topology", label: "Topology", icon: Waypoints },
  { id: "recordings", label: "Recordings", icon: Clapperboard },
  { id: "containers", label: "Containers", icon: Boxes },
  { id: "resident-sessions", label: "Resident Sessions", icon: Timer },
] as const;

/** A module id, derived from the registry so the two cannot drift. */
export type ModuleId = (typeof MODULES)[number]["id"];

// Sidebar display structure: four cockpits, then Help pinned to the bottom of
// the rail (`pinBottom` + an empty title — App.tsx pushes that group with
// `mt-auto` and skips the label). Group titles are i18n keys rendered through
// t() in App.tsx. Order inside a group is the rail order, and therefore the
// Ctrl/⌘+1..9 order: shortcuts follow the rail.
export const SIDEBAR_GROUPS: ReadonlyArray<{
  title: string;
  modules: ModuleId[];
  pinBottom?: boolean;
}> = [
  {
    title: "Review",
    modules: ["approvals", "dashboard", "audit", "recordings", "activity"],
  },
  {
    title: "Operations",
    modules: ["terminal", "resident-sessions", "execute", "playbooks", "tunnels", "files-sessions"],
  },
  {
    title: "Fleet",
    modules: ["hosts", "topology", "containers", "proxies", "keys"],
  },
  { title: "System", modules: ["mcp-agents", "sync", "config"] },
  { title: "", modules: ["help"], pinBottom: true },
];

// Flat sidebar render order, derived from the groups.
export const SIDEBAR_ORDER: ModuleId[] = SIDEBAR_GROUPS.flatMap((group) => group.modules);

// Ctrl/⌘+1..9. The rail is grouped by cockpit, which pushes Hosts past the
// ninth row, so the shortcuts are this daily set instead of the rail order.
export const SHORTCUT_MODULES: ModuleId[] = [
  "approvals",
  "dashboard",
  "terminal",
  "execute",
  "hosts",
  "files-sessions",
  "tunnels",
  "audit",
  "activity",
];

// V3-4: mesh navigation, not a tab trail — each module lists a handful of the
// other modules an operator is likely to jump to from here.
export const RELATED_MODULES: Partial<Record<ModuleId, ModuleId[]>> = {
  hosts: ["execute", "files-sessions", "tunnels", "terminal", "audit", "topology"],
  topology: ["hosts", "tunnels"],
  proxies: ["hosts"],
  terminal: ["hosts"],
  recordings: ["terminal", "audit"],
  execute: ["hosts", "audit"],
  "files-sessions": ["hosts"],
  tunnels: ["hosts"],
  activity: ["audit"],
  keys: ["hosts"],
  playbooks: ["hosts", "execute"],
  audit: ["hosts", "approvals"],
  approvals: ["execute", "audit"],
  sync: ["config"],
  config: ["sync"],
  containers: ["terminal", "hosts"],
  "resident-sessions": ["terminal", "containers"],
};
