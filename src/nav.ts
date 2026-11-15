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
 * `MODULES` order is frozen because the Ctrl/⌘+1..9 shortcuts index into it
 * (`App.tsx`); new modules are appended, never inserted.
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
  { id: "mcp-agents", label: "MCP Agents", icon: Bot },
  { id: "sync", label: "Sync", icon: Cloud },
  { id: "keys", label: "Keys", icon: Key },
  { id: "playbooks", label: "Playbooks", icon: BookOpen },
  { id: "audit", label: "Audit", icon: History },
  { id: "help", label: "Help", icon: HelpCircle },
  // V2-2/V4-3: appended (not inserted) so the existing Ctrl/⌘+1..9 module
  // shortcuts (V2-5) keep pointing at the same modules.
  { id: "approvals", label: "Approvals", icon: ShieldCheck },
  { id: "config", label: "Config Snapshots", icon: Camera },
  { id: "topology", label: "Topology", icon: Waypoints },
  { id: "recordings", label: "Recordings", icon: Clapperboard },
  { id: "containers", label: "Containers", icon: Boxes },
  { id: "resident-sessions", label: "Resident Sessions", icon: Timer },
] as const;

/** A module id, derived from the registry so the two cannot drift. */
export type ModuleId = (typeof MODULES)[number]["id"];

// Sidebar display structure: approvals pinned above the groups (its
// pending-count badge must stay the first thing in the rail), the rest in four
// functional groups so twenty entries scan by domain instead of by position.
// Group titles are i18n keys rendered through t() in App.tsx; the empty title
// marks the pinned group.
export const SIDEBAR_GROUPS: ReadonlyArray<{ title: string; modules: ModuleId[] }> = [
  { title: "", modules: ["approvals"] },
  { title: "Operations", modules: ["hosts", "terminal", "execute", "files-sessions", "tunnels", "playbooks"] },
  { title: "Environments", modules: ["containers", "resident-sessions"] },
  {
    title: "Observability",
    modules: ["dashboard", "topology", "activity", "audit", "recordings"],
  },
  { title: "System", modules: ["mcp-agents", "proxies", "keys", "sync", "config", "help"] },
];

// v3 3.1: flat sidebar render order, now derived from the groups so the
// set-equality invariants in `nav.test.ts` keep holding no matter how the
// groups are arranged. MODULES order above is frozen by the Ctrl/⌘+1..9
// shortcuts; only the sidebar display order changes.
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
