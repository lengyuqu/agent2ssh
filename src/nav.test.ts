import { describe, expect, it } from "vitest";
import { zh } from "./i18n";
import { MODULES, RELATED_MODULES, SIDEBAR_GROUPS, SIDEBAR_ORDER } from "./nav";

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
    // The first group carries the approvals pin and must stay titleless so the
    // pending-count badge sits at the very top of the rail; the dashboard (the
    // page the console opens on) sits right under it.
    expect(SIDEBAR_GROUPS[0]).toEqual({ title: "", modules: ["approvals", "dashboard"] });
    // Help is pinned to the bottom of the rail.
    expect(SIDEBAR_GROUPS[SIDEBAR_GROUPS.length - 1]).toEqual({ title: "", modules: ["help"], pinBottom: true });
    for (const group of SIDEBAR_GROUPS) {
      if (!group.title) continue;
      expect(zh[group.title], `missing zh title for ${group.title}`).toBeTruthy();
    }
  });

  it("maps Ctrl/⌘+1..9 to the first nine sidebar entries", () => {
    // App.tsx indexes SIDEBAR_ORDER for the numeric shortcuts, so Ctrl/⌘+N is
    // always the Nth item a user sees in the rail. Pinned here so a regroup
    // that moves a shortcut is a visible, deliberate change.
    expect(SIDEBAR_ORDER.slice(0, 9)).toEqual([
      "approvals",
      "dashboard",
      "hosts",
      "proxies",
      "terminal",
      "resident-sessions",
      "execute",
      "files-sessions",
      "tunnels",
    ]);
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
