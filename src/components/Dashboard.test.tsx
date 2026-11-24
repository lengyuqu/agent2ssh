import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DaemonHealth, HostProfile } from "../types";
import Dashboard from "./Dashboard";
import { ToastProvider } from "./ui/toast";

const mockApi = vi.hoisted(() => ({
  listAudit: vi.fn(),
  pingHosts: vi.fn(),
}));

vi.mock("../api", () => ({ api: mockApi, reportError: vi.fn() }));
vi.mock("../i18n", () => ({ useI18n: () => ({ t: (text: string) => text }) }));
vi.mock("../eventsBus", () => ({
  useAgentEvents: () => {},
  useEventsStatus: () => "live",
}));

function host(name: string): HostProfile {
  return { name, host: "10.0.0.1", user: "deploy", group: "default" };
}

function renderDashboard(overrides: Partial<ComponentProps<typeof Dashboard>> = {}) {
  const props: ComponentProps<typeof Dashboard> = {
    hosts: [host("web")],
    connectionStatuses: [],
    pendingApprovalsCount: 2,
    secretsLocked: true,
    daemonHealth: { ok: false },
    onNavigateModule: vi.fn(),
    onRequestUnlock: vi.fn(),
    onStartDaemon: vi.fn().mockResolvedValue(undefined),
    onOpenAnomalies: vi.fn(),
    onOpenHostTerminal: vi.fn(),
    onInspectHost: vi.fn(),
    ...overrides,
  };
  const view = render(
    <ToastProvider>
      <Dashboard {...props} />
    </ToastProvider>
  );
  return { ...view, props };
}

function executionsCard() {
  return screen.getByRole("button", { name: /24h executions/ });
}

describe("Dashboard", () => {
  beforeEach(() => {
    mockApi.listAudit.mockReset();
    mockApi.listAudit.mockResolvedValue([]);
    mockApi.pingHosts.mockReset();
  });

  it("runs an action for each of the six cards", async () => {
    const { props } = renderDashboard();
    await waitFor(() => expect(mockApi.listAudit).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: /Host health/ }));
    fireEvent.click(screen.getByRole("button", { name: /Pending approvals/ }));
    fireEvent.click(screen.getByRole("button", { name: /Anomaly alerts/ }));
    fireEvent.click(executionsCard());
    fireEvent.click(screen.getByRole("button", { name: /Credential store/ }));
    fireEvent.click(screen.getByRole("button", { name: /Local daemon/ }));

    expect(props.onNavigateModule).toHaveBeenCalledWith("hosts");
    expect(props.onNavigateModule).toHaveBeenCalledWith("approvals");
    expect(props.onOpenAnomalies).toHaveBeenCalledTimes(1);
    expect(props.onNavigateModule).toHaveBeenCalledWith("audit");
    expect(props.onRequestUnlock).toHaveBeenCalledTimes(1);
    expect(props.onNavigateModule).not.toHaveBeenCalledWith("keys");
    expect(props.onStartDaemon).toHaveBeenCalledTimes(1);
    expect(props.onNavigateModule).not.toHaveBeenCalledWith("activity");
  });

  it("opens keys when unlocked and activity when the daemon is online", async () => {
    mockApi.listAudit.mockResolvedValue([{}, {}, {}, {}]);
    const { props } = renderDashboard({
      secretsLocked: false,
      daemonHealth: { ok: true, version: "9.9.9" } satisfies DaemonHealth,
    });

    await waitFor(() => expect(executionsCard().textContent).toContain("4"));
    expect(executionsCard().textContent).not.toContain("—");
    expect(executionsCard().textContent).not.toContain("Unavailable");

    fireEvent.click(screen.getByRole("button", { name: /Credential store/ }));
    expect(props.onRequestUnlock).not.toHaveBeenCalled();
    expect(props.onNavigateModule).toHaveBeenCalledWith("keys");

    const daemon = screen.getByRole("button", { name: /Local daemon/ });
    expect(daemon.textContent).toContain("Daemon running");
    expect(daemon.textContent).toContain("v9.9.9");
    fireEvent.click(daemon);
    expect(props.onNavigateModule).toHaveBeenCalledWith("activity");
    expect(props.onStartDaemon).not.toHaveBeenCalled();
  });

  it("shows a starting state while the offline daemon is starting", async () => {
    let finish = () => {};
    const onStartDaemon = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        })
    );
    renderDashboard({ onStartDaemon });
    await waitFor(() => expect(mockApi.listAudit).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: /Local daemon/ }));
    expect(screen.getByRole("button", { name: /Local daemon/ }).textContent).toContain("Starting...");
    fireEvent.click(screen.getByRole("button", { name: /Local daemon/ }));
    expect(onStartDaemon).toHaveBeenCalledTimes(1);

    await act(async () => {
      finish();
    });
    expect(screen.getByRole("button", { name: /Local daemon/ }).textContent).toContain("Daemon offline");
  });

  it("renders an em dash instead of an ellipsis when the audit read fails", async () => {
    let rejectAudit: (reason?: unknown) => void = () => {};
    mockApi.listAudit.mockReturnValue(
      new Promise((_resolve, reject) => {
        rejectAudit = reject;
      })
    );
    renderDashboard({ daemonHealth: { ok: true, version: "1.0.0" } });

    expect(executionsCard().textContent).toContain("…");
    expect(executionsCard().textContent).not.toContain("Unavailable");

    await act(async () => {
      rejectAudit(new Error("audit offline"));
    });

    await waitFor(() => expect(executionsCard().textContent).toContain("Unavailable"));
    expect(executionsCard().textContent).toContain("—");
    expect(executionsCard().textContent).not.toContain("…");
  });

  it("renders an em dash while the daemon is offline, before the audit read settles", () => {
    mockApi.listAudit.mockReturnValue(new Promise(() => {}));
    renderDashboard({ daemonHealth: { ok: false } });

    expect(executionsCard().textContent).toContain("—");
    expect(executionsCard().textContent).toContain("Unavailable");
    expect(executionsCard().textContent).not.toContain("…");
  });
});
