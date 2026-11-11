import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { HostPolicySummary } from "../types";
import HostPolicyLine from "./HostPolicyLine";

const fetchHostPolicy = vi.fn();

vi.mock("../api", () => ({
  api: {
    fetchHostPolicy: (host: string) => fetchHostPolicy(host),
    setTrayLabels: vi.fn().mockResolvedValue(undefined),
  },
  reportError: vi.fn(),
}));

function renderLine(host: string | null) {
  render(
    <I18nProvider>
      <HostPolicyLine host={host} />
    </I18nProvider>
  );
}

function summary(overrides: Partial<HostPolicySummary> = {}): HostPolicySummary {
  return {
    host: "web-1",
    ai_allowed: true,
    risk_override: null,
    low: "auto",
    medium: "auto",
    high: "required",
    policies: [],
    blocked_patterns: [],
    ...overrides,
  };
}

describe("HostPolicyLine", () => {
  it("renders the real default when no summary is available", () => {
    // No host selected, or the daemon is unreachable: this is the fallback
    // branch. With no policy configured, `authorize_command_with_approval` only
    // gates high risk (`needs_approval = policy.is_some() || (risk == High &&
    // !force)`), so medium is *not* "need approval" -- the old copy said it was.
    fetchHostPolicy.mockResolvedValue(null);
    renderLine(null);

    expect(screen.getByText("low/med auto-approve")).toBeTruthy();
    expect(screen.getByText("high need approval")).toBeTruthy();
    expect(screen.getByText("deletion forbidden")).toBeTruthy();
    expect(screen.queryByText("med/high need approval")).toBeNull();
  });

  it("shows the live summary's host and per-risk modes when the daemon answers", async () => {
    fetchHostPolicy.mockResolvedValue(summary({ host: "db-2", medium: "required" }));
    renderLine("db-2");

    await waitFor(() => expect(fetchHostPolicy).toHaveBeenCalledWith("db-2"));
    expect(await screen.findByText("db-2")).toBeTruthy();
    expect(screen.getByText("med need approval")).toBeTruthy();
    expect(screen.queryByText("low/med auto-approve")).toBeNull();
  });
});
