import { describe, expect, it } from "vitest";
import { zh } from "./i18n";
import { MODULES, RELATED_MODULES, SHORTCUT_MODULES, SIDEBAR_GROUPS, SIDEBAR_ORDER } from "./nav";

/**
 * The module registry has several lists that must agree, and a missing entry
 * is silent: `resident-sessions` shipped with a finished panel, an i18n label
 * and a render arm, but no `SIDEBAR_ORDER` entry, so nobody could open it.
 * These assertions are the guard that makes that class of gap impossible.
 */
describe("module registry", () => {
  const ids = MODULES.map((module) => module.id);

  it("gives every module its own id and label", () => {
    expect(new Set(ids).size).toBe(ids.length);
    const labels = MODULES.map((module) => module.label);
    expect(new Set(labels).size).toBe(labels.length);
    for (const label of labels) {
      expect(label.length).toBeGreaterThan(0);
    }
  });

  it("offers every module in the sidebar exactly once", () => {
    // Set equality in both directions: a module missing from the sidebar is
    // unreachable, and an id that is not a module is a typo that renders
    // nothing.
    expect([...SIDEBAR_ORDER].sort()).toEqual([...ids].sort());
    expect(new Set(SIDEBAR_ORDER).size).toBe(SIDEBAR_ORDER.length);
  });

  it("covers every module in exactly one sidebar group, with titles translated", () => {
    // The grouped rail is rendered from SIDEBAR_GROUPS; the flat SIDEBAR_ORDER
    // is derived from it, so the groups are the real source. A module missing
    // here is unreachable exactly as a missing SIDEBAR_ORDER entry used to be,
    // and a duplicated id would render the same panel under two groups.
    const grouped = SIDEBAR_GROUPS.flatMap((group) => group.modules);
    expect([...grouped].sort()).toEqual([...ids].sort());
    expect(new Set(grouped).size).toBe(grouped.length);
    // Four cockpits, in rail order. Approvals stays first inside Review so its
    // pending-count badge is the top of the rail. Help is its own titleless
    // group pinned to the bottom.
    expect(SIDEBAR_GROUPS).toEqual([
      { title: "Review", modules: ["approvals", "dashboard", "audit", "recordings", "activity"] },
      {
        title: "Operations",
        modules: ["terminal", "resident-sessions", "execute", "playbooks", "tunnels", "files-sessions"],
      },
      { title: "Fleet", modules: ["hosts", "topology", "containers", "proxies", "keys"] },
      { title: "System", modules: ["mcp-agents", "sync", "config"] },
      { title: "", modules: ["help"], pinBottom: true },
    ]);
    expect(zh["Review"]).toBe("审查与态势");
    expect(zh["Operations"]).toBe("终端与运维");
    expect(zh["Fleet"]).toBe("资产与网络");
    expect(zh["System"]).toBe("系统与配置");
    for (const group of SIDEBAR_GROUPS) {
      if (!group.title) continue;
      expect(zh[group.title], `missing zh title for ${group.title}`).toBeTruthy();
    }
  });

  it("maps Ctrl/⌘+1..9 to the daily modules, not the rail order", () => {
    // The cockpits keep related modules together, so the first nine rows are
    // not the nine an operator hits most. App.tsx indexes SHORTCUT_MODULES.
    expect(SHORTCUT_MODULES).toEqual([
      "approvals",
      "dashboard",
      "terminal",
      "execute",
      "hosts",
      "files-sessions",
      "tunnels",
      "audit",
      "activity",
    ]);
    expect(new Set(SHORTCUT_MODULES).size).toBe(9);
    for (const id of SHORTCUT_MODULES) expect(ids).toContain(id);
  });

  it("relates only known modules, never to itself", () => {
    for (const [from, to] of Object.entries(RELATED_MODULES)) {
      expect(ids).toContain(from);
      expect(to?.length).toBeTruthy();
      for (const target of to ?? []) {
        expect(ids).toContain(target);
        expect(target).not.toBe(from);
      }
    }
  });

  it("has a Chinese label for every module", () => {
    // The sidebar renders the label through t(), which falls back to the key:
    // a missing entry shows English in a Chinese UI rather than failing loudly.
    for (const module of MODULES) {
      expect(zh[module.label], `missing zh label for ${module.label}`).toBeTruthy();
    }
  });
});
