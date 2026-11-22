import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { MODULES } from "../nav";
import type { ApprovalRequest } from "../types";
import CommandPalette from "./CommandPalette";

const pending: ApprovalRequest[] = [
  {
    id: "a1",
    host: "db-primary",
    command: "systemctl restart postgresql",
    risk_level: "high",
    requested_at: "2026-11-22T09:00:00Z",
    ttl_secs: 300,
    status: "pending",
  },
];

function renderPalette(hostCount = 1) {
  const onNavigateModule = vi.fn();
  const onOpenApprovals = vi.fn();
  const onClose = vi.fn();
  const hosts = Array.from({ length: hostCount }, (_, index) => ({
    name: index === 0 ? "zos" : `web-${String(index).padStart(2, "0")}`,
    host: index === 0 ? "zos.example.test" : `web${index}.example.test`,
    user: "operator",
    group: "default",
  }));
  render(
    <I18nProvider>
      <CommandPalette
        open
        modules={MODULES}
        hosts={hosts}
        pending={pending}
        onClose={onClose}
        onNavigateModule={onNavigateModule}
        onSelectHost={vi.fn()}
        onOpenApprovals={onOpenApprovals}
      />
    </I18nProvider>
  );
  const input = screen.getByPlaceholderText("Search modules, hosts, tools... (> for MCP tools)");
  return { input, onNavigateModule, onOpenApprovals, onClose };
}

function pressEnter(input: HTMLElement) {
  fireEvent.keyDown(input, { key: "Enter" });
}

describe("CommandPalette", () => {
  it("lists every sidebar module in the navigate group", () => {
    renderPalette();
    for (const module of MODULES) {
      expect(screen.getByRole("button", { name: new RegExp(module.label) })).toBeTruthy();
    }
  });

  it("pins a pending approval on top and opens the workbench on enter", () => {
    const { input, onOpenApprovals, onNavigateModule } = renderPalette();
    pressEnter(input);
    expect(onOpenApprovals).toHaveBeenCalledTimes(1);
    expect(onNavigateModule).not.toHaveBeenCalled();
  });

  // The pinned card used to ignore the query, so typing a module name and
  // pressing ↵ landed on the approval instead of the only matching row.
  it("leaves enter on the matching module when a query is typed", () => {
    const { input, onNavigateModule, onOpenApprovals } = renderPalette();
    fireEvent.change(input, { target: { value: "Playbooks" } });
    expect(screen.queryByText("systemctl restart postgresql")).toBeNull();
    pressEnter(input);
    expect(onNavigateModule).toHaveBeenCalledWith("playbooks");
    expect(onOpenApprovals).not.toHaveBeenCalled();
  });

  it("keeps an approval whose own command matches the query", () => {
    const { input, onOpenApprovals } = renderPalette();
    fireEvent.change(input, { target: { value: "postgresql" } });
    expect(screen.getByText("systemctl restart postgresql")).toBeTruthy();
    pressEnter(input);
    expect(onOpenApprovals).toHaveBeenCalledTimes(1);
  });

  // A single global budget let the modules and the unfiltered MCP list spend
  // all of it, dropping the host group off the unsearched palette entirely.
  it("shows every group at once with the full module list and a large host list", () => {
    renderPalette(40);
    for (const heading of ["Pending approvals", "Navigate", "MCP tools", "Connect host"]) {
      expect(screen.getByText(heading)).toBeTruthy();
    }
    expect(screen.getByText("zos")).toBeTruthy();
    expect(screen.getAllByText(/^web-/).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: /ssh_list_hosts/ })).toBeTruthy();
  });

  it("narrows to MCP tools with the > prefix", () => {
    const { input } = renderPalette(40);
    fireEvent.change(input, { target: { value: ">" } });
    expect(screen.queryByText("Navigate")).toBeNull();
    expect(screen.queryByText("Connect host")).toBeNull();
    expect(screen.getByText("MCP tools")).toBeTruthy();
  });
});