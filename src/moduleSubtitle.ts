import type { ModuleId } from "./nav";

export type SubtitleContext = {
  pending: number;
  totalHosts: number;
  connectedHosts: number;
  selectedHost: string;
  /** Connection string for the selected host on the hosts page. Shown as-is. */
  hostSummary: string | null;
};

export type ModuleSubtitle = {
  text: string;
  vars?: Record<string, string | number>;
  /** Connection strings are not dictionary keys. */
  raw?: boolean;
};

/** Page-header line for one module. Never the generic host count. */
export function moduleSubtitle(module: ModuleId, ctx: SubtitleContext): ModuleSubtitle {
  switch (module) {
    case "approvals":
      return ctx.pending > 0
        ? { text: "{pending} pending approvals", vars: { pending: ctx.pending } }
        : { text: "No pending requests" };
    case "hosts":
      if (ctx.hostSummary) return { text: ctx.hostSummary, raw: true };
      if (ctx.totalHosts === 0) return { text: "Add a host to start issuing SSH commands" };
      return {
        text: "{connected}/{total} hosts connected",
        vars: { connected: ctx.connectedHosts, total: ctx.totalHosts },
      };
    case "execute":
    case "playbooks":
      return ctx.selectedHost
        ? { text: "Target: {host}", vars: { host: ctx.selectedHost } }
        : { text: "Single, multi-host, and playbooks in one workbench" };
    case "files-sessions":
      return ctx.selectedHost
        ? { text: "Current target: {host}", vars: { host: ctx.selectedHost } }
        : { text: "Choose a host to browse files" };
    case "tunnels":
      return { text: "Port forwards and live traffic" };
    case "audit":
      return { text: "Command history, risk, and replay" };
    case "recordings":
      return { text: "Local terminal recordings" };
    case "activity":
      return { text: "Live agent and execution events" };
    case "sync":
      return { text: "WebDAV sync and remote copies" };
    case "config":
      return { text: "Saved configuration snapshots" };
    case "mcp-agents":
      return { text: "MCP servers and local CLI clients" };
    case "containers":
      return { text: "Local container runtimes" };
    case "resident-sessions":
      return { text: "Shells that stay open between commands" };
    case "topology":
      return { text: "How hosts, jumps, and proxies connect" };
    case "proxies":
      return { text: "Jump proxies used by hosts" };
    case "keys":
      return { text: "SSH keys and the hosts that use them" };
    case "help":
      return { text: "Guides for this console" };
    case "terminal":
      return { text: "Interactive sessions" };
    case "dashboard":
      return { text: "Health, approvals, and what needs attention" };
    default:
      return { text: "Health, approvals, and what needs attention" };
  }
}
