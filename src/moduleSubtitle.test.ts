import { describe, expect, it } from "vitest";
import { MODULES } from "./nav";
import { moduleSubtitle, type SubtitleContext } from "./moduleSubtitle";

const base: SubtitleContext = {
  pending: 0,
  totalHosts: 4,
  connectedHosts: 2,
  selectedHost: "",
  hostSummary: null,
};

describe("moduleSubtitle", () => {
  it("does not reuse the generic host count for every module", () => {
    for (const module of MODULES) {
      const line = moduleSubtitle(module.id, base);
      expect(line.text).not.toBe("{count} hosts configured");
    }
  });

  it("counts pending approvals and falls back when the queue is empty", () => {
    expect(moduleSubtitle("approvals", { ...base, pending: 3 })).toEqual({
      text: "{pending} pending approvals",
      vars: { pending: 3 },
    });
    expect(moduleSubtitle("approvals", base).text).toBe("No pending requests");
  });

  it("shows the selected host connection string, otherwise how many are up", () => {
    expect(
      moduleSubtitle("hosts", { ...base, hostSummary: "deploy@10.0.0.1:22" }),
    ).toEqual({ text: "deploy@10.0.0.1:22", raw: true });
    expect(moduleSubtitle("hosts", base)).toEqual({
      text: "{connected}/{total} hosts connected",
      vars: { connected: 2, total: 4 },
    });
    expect(moduleSubtitle("hosts", { ...base, totalHosts: 0 }).text).toBe(
      "Add a host to start issuing SSH commands",
    );
  });

  it("names the execution target, or the workbench when none is selected", () => {
    expect(moduleSubtitle("execute", { ...base, selectedHost: "web-01" }).text).toBe(
      "Target: {host}",
    );
    expect(moduleSubtitle("playbooks", base).text).toBe(
      "Single, multi-host, and playbooks in one workbench",
    );
    expect(moduleSubtitle("files-sessions", { ...base, selectedHost: "web-01" }).vars).toEqual({
      host: "web-01",
    });
    expect(moduleSubtitle("files-sessions", base).text).toBe("Choose a host to browse files");
  });
});
