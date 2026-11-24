import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AuditEntry } from "../types";
import AuditWorkbench from "./AuditWorkbench";
import { ToastProvider } from "./ui/toast";

const listAudit = vi.hoisted(() => vi.fn());

vi.mock("../api", () => ({
  api: {
    listAudit,
    clearAudit: vi.fn(),
  },
  reportError: vi.fn(),
}));

vi.mock("../eventsBus", () => ({
  useAgentEvents: () => {},
}));

vi.mock("../i18n", () => ({
  useI18n: () => ({
    t: (text: string, vars?: Record<string, string | number>) => {
      if (!vars) return text;
      return Object.entries(vars).reduce(
        (next, [key, value]) => next.split(`{${key}}`).join(String(value)),
        text,
      );
    },
  }),
}));

if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
}

function entry(overrides: Partial<AuditEntry> & Pick<AuditEntry, "id" | "host" | "command" | "risk_level">): AuditEntry {
  return {
    ts: new Date().toISOString(),
    exit_code: 0,
    duration_ms: 15,
    source: "cli",
    decision: "auto_allowed",
    ...overrides,
  };
}

const ENTRIES: AuditEntry[] = [
  entry({ id: "low-1", host: "prod-1", command: "uptime", risk_level: "low", source: "cli" }),
  entry({ id: "med-1", host: "prod-1", command: "df -h", risk_level: "medium", source: "mcp" }),
  entry({
    id: "high-1",
    host: "db-1",
    command: "rm -rf /tmp/x",
    risk_level: "high",
    source: "mcp",
    original_command: "rm -rf /",
    decided_by: "human",
    reason: "rewritten by approver",
    change_id: "CHG-9",
    decision: "approved_rewritten",
    duration_ms: 40,
  }),
  entry({ id: "blocked-1", host: "db-1", command: "shutdown -h now", risk_level: "blocked", source: "desktop" }),
];

function renderWorkbench(props: { onReplay?: (entry: AuditEntry) => void; onEditAndExecute?: (entry: AuditEntry) => void } = {}) {
  const onReplay = props.onReplay ?? vi.fn();
  const onEditAndExecute = props.onEditAndExecute ?? vi.fn();
  render(
    <ToastProvider>
      <AuditWorkbench onReplay={onReplay} onEditAndExecute={onEditAndExecute} />
    </ToastProvider>,
  );
  return { onReplay, onEditAndExecute };
}

beforeEach(() => {
  listAudit.mockReset();
  listAudit.mockResolvedValue(ENTRIES);
});

describe("AuditWorkbench", () => {
  it("keeps other risk counts after selecting high", async () => {
    renderWorkbench();
    const high = await screen.findByRole("button", { name: "high 1" });
    expect(listAudit).toHaveBeenCalledTimes(1);
    fireEvent.click(high);
    expect(listAudit).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "low 1" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "medium 1" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "blocked 1" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "all 4" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "high 1", pressed: true })).toBeTruthy();
    expect(screen.getByText("rm -rf /tmp/x")).toBeTruthy();
    expect(screen.queryByText("uptime")).toBeNull();
    expect(screen.queryByText("df -h")).toBeNull();
  });

  it("keeps the risk and host filters when refreshing", async () => {
    renderWorkbench();
    await screen.findByRole("button", { name: "high 1" });
    fireEvent.click(screen.getByRole("button", { name: "high 1" }));
    fireEvent.click(screen.getByRole("button", { name: "db-1" }));
    expect(screen.getByRole("button", { name: "Host: db-1" })).toBeTruthy();
    expect(screen.queryByText("uptime")).toBeNull();
    expect(screen.queryByText("shutdown -h now")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Refresh audit" }));
    await waitFor(() => expect(listAudit.mock.calls.length).toBeGreaterThanOrEqual(2));

    expect(screen.getByRole("button", { name: "high 1", pressed: true })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Host: db-1" })).toBeTruthy();
    expect(screen.getByText("rm -rf /tmp/x")).toBeTruthy();
    expect(screen.queryByText("uptime")).toBeNull();
    expect(screen.queryByText("shutdown -h now")).toBeNull();
    const calls = listAudit.mock.calls;
    const lastFilter = calls[calls.length - 1]?.[0] as { risk_level?: string; host?: string } | undefined;
    expect(lastFilter?.risk_level).toBeUndefined();
    expect(lastFilter?.host).toBeUndefined();
  });

  it("filters the table when a host or a source is selected", async () => {
    renderWorkbench();
    await screen.findByText("uptime");

    fireEvent.click(screen.getByRole("button", { name: "mcp" }));
    expect(screen.getByText("df -h")).toBeTruthy();
    expect(screen.getByText("rm -rf /tmp/x")).toBeTruthy();
    expect(screen.queryByText("uptime")).toBeNull();
    expect(screen.queryByText("shutdown -h now")).toBeNull();
    expect(screen.getByRole("button", { name: "Source: mcp" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "mcp" }));
    expect(screen.getByText("uptime")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "prod-1" }));
    expect(screen.getByText("uptime")).toBeTruthy();
    expect(screen.getByText("df -h")).toBeTruthy();
    expect(screen.queryByText("rm -rf /tmp/x")).toBeNull();
    expect(screen.queryByText("shutdown -h now")).toBeNull();
  });

  it("opens a detail drawer with the original command contrast", async () => {
    const { onReplay } = renderWorkbench();
    const { row } = await rowForCommand("rm -rf /tmp/x");

    fireEvent.click(within(row).getByRole("checkbox"));
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(within(row).getByRole("button", { name: "Replay" }));
    expect(onReplay).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog")).toBeNull();

    const live = (await rowForCommand("rm -rf /tmp/x")).command;
    fireEvent.click(live);
    const dialog = await screen.findByRole("dialog", { name: "Audit detail" });
    expect(within(dialog).getByText("Original request")).toBeTruthy();
    expect(within(dialog).getByText("rm -rf /")).toBeTruthy();
    expect(within(dialog).getByText("Executed command")).toBeTruthy();
    expect(within(dialog).getByText("rm -rf /tmp/x")).toBeTruthy();
    expect(within(dialog).getByText("CHG-9")).toBeTruthy();
    expect(within(dialog).getByText("human")).toBeTruthy();
    expect(within(dialog).getByText("rewritten by approver")).toBeTruthy();
  });

  it("calls edit-in-execution with the host and command", async () => {
    const onEditAndExecute = vi.fn();
    renderWorkbench({ onEditAndExecute });
    const { command } = await rowForCommand("rm -rf /tmp/x");
    fireEvent.click(command);
    const dialog = await screen.findByRole("dialog", { name: "Audit detail" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Edit in Execution" }));
    expect(onEditAndExecute).toHaveBeenCalledTimes(1);
    expect(onEditAndExecute).toHaveBeenCalledWith(
      expect.objectContaining({ host: "db-1", command: "rm -rf /tmp/x" }),
    );
  });
});

/** The table cell text node is replaced when the audit list commits, so read it from the live table. */
async function rowForCommand(text: string) {
  await screen.findByText(text);
  const command = [...document.querySelectorAll("table code")].find((node) => node.textContent === text);
  const row = command?.closest("tr");
  if (!command || !row) throw new Error(`missing audit row for ${text}`);
  return { command, row };
}
