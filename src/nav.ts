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
 * `SIDEBAR_GROUPS`, and the Ctrl/⌘+1..9 shortcuts index into `SIDEBAR_ORDER`
 * so "the Nth item in the rail" and "Ctrl/⌘+N" are always the same module.
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

// Sidebar display structure. The top pinned group holds approvals (its
// pending-count badge must stay the first thing in the rail) and the dashboard
// (the page the console opens on, so the active highlight starts at the top).
// The rest sit in three functional groups so twenty entries scan by domain
// instead of by position, and Help is pinned to the bottom of the rail.
// Group titles are i18n keys rendered through t() in App.tsx; an empty title
// marks a pinned group.
export const SIDEBAR_GROUPS: ReadonlyArray<{
  title: string;
  modules: ModuleId[];
  pinBottom?: boolean;
}> = [
  { title: "", modules: ["approvals", "dashboard"] },
  {
    title: "Operations",
    modules: [
      "hosts",
      "proxies",
      "terminal",
      "resident-sessions",
      "execute",
      "files-sessions",
      "tunnels",
      "playbooks",
      "containers",
    ],
  },
  { title: "Observability", modules: ["activity", "audit", "recordings", "topology"] },
  { title: "System", modules: ["mcp-agents", "keys", "sync", "config"] },
  { title: "", modules: ["help"], pinBottom: true },
];

// Flat sidebar render order, derived from the groups. The Ctrl/⌘+1..9
// shortcuts index into it (App.tsx), so they always follow what the rail shows.
export const SIDEBAR_ORDER: ModuleId[] = SIDEBAR_GROUPS.flatMap((group) => group.modules);

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
