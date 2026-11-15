import { describe, expect, it } from "vitest";
import {
  closeTab,
  openHostTab,
  pruneMissingHosts,
  type WorkspaceTab,
} from "./types";

function tab(host: string, id = `ws-${host}`): WorkspaceTab {
  return { id, host, createdAt: 0 };
}

describe("workspace tab transitions", () => {
  it("opens a host once and focuses the existing tab on repeat clicks", () => {
    const first = openHostTab([], null, "zos");
    expect(first.tabs.map((tab) => tab.host)).toEqual(["zos"]);
    expect(first.activeTabId).toBe(first.tabs[0].id);

    const second = openHostTab(first.tabs, first.activeTabId, "zos");
    expect(second.tabs).toBe(first.tabs); // same array — no duplicate tab
    expect(second.activeTabId).toBe(first.activeTabId);
  });

  it("appends a second host without disturbing the active tab", () => {
    const state = openHostTab([tab("a", "ws-a")], "ws-a", "b");
    expect(state.tabs.map((tab) => tab.host)).toEqual(["a", "b"]);
    // The new tab takes focus, identified by its generated id.
    expect(state.tabs[1].id).toBe(state.activeTabId);
    expect(state.tabs[0].id).toBe("ws-a");
  });

  it("closing the active tab moves focus to the right neighbor, else the left", () => {
    const tabs = [tab("a", "ws-a"), tab("b", "ws-b"), tab("c", "ws-c")];
    // Active "b": right neighbor "c" takes focus.
    expect(closeTab(tabs, "ws-b", "ws-b")).toEqual({
      tabs: [tabs[0], tabs[2]],
      activeTabId: "ws-c",
    });
    // Active "c" (last): left neighbor takes focus.
    expect(closeTab(tabs, "ws-c", "ws-c")).toEqual({
      tabs: [tabs[0], tabs[1]],
      activeTabId: "ws-b",
    });
    // Closing an inactive tab keeps the active tab untouched.
    expect(closeTab(tabs, "ws-a", "ws-b")).toEqual({
      tabs: [tabs[0], tabs[2]],
      activeTabId: "ws-a",
    });
    // Closing the last tab leaves an empty workspace.
    expect(closeTab([tab("a", "ws-a")], "ws-a", "ws-a")).toEqual({
      tabs: [],
      activeTabId: null,
    });
    // Unknown id is a no-op.
    expect(closeTab(tabs, "ws-a", "ws-nope")).toEqual({ tabs, activeTabId: "ws-a" });
  });

  it("prunes tabs whose host disappeared from the configuration", () => {
    const tabs = [tab("a", "ws-a"), tab("gone", "ws-gone"), tab("c", "ws-c")];
    expect(pruneMissingHosts(tabs, new Set(["a", "c"]))).toEqual([tabs[0], tabs[2]]);
  });
});
