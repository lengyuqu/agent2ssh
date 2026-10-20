import { describe, expect, it } from "vitest";
import { zh } from "./i18n";
import { MODULES, RELATED_MODULES, SIDEBAR_ORDER } from "./nav";

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

  it("keeps the first nine modules stable for the Ctrl/⌘+1..9 shortcuts", () => {
    // App.tsx indexes MODULES for the numeric shortcuts, so inserting a module
    // in the middle silently repoints a shortcut. Appending is the contract.
    expect(MODULES.slice(0, 9).map((module) => module.id)).toEqual([
      "dashboard",
      "hosts",
      "proxies",
      "terminal",
      "execute",
      "files-sessions",
      "tunnels",
      "activity",
      "mcp-agents",
    ]);
  });

  it("relates only known modules, never to itself", () => {
    for (const [from, to] of Object.entries(RELATED_MODULES)) {
      expect(ids).toContain(from);
      expect(to && to.length).toBeTruthy();
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
