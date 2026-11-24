import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import ConnectionTopology from "./ConnectionTopology";

const mockApi = vi.hoisted(() => ({
  setTrayLabels: vi.fn().mockResolvedValue(undefined),
  forwardList: vi.fn().mockResolvedValue([]),
}));

vi.mock("../api", () => ({
  api: mockApi,
  reportError: vi.fn(),
}));

describe("ConnectionTopology node inspector", () => {
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem("agent2ssh.language", "en");
    vi.clearAllMocks();
    mockApi.forwardList.mockResolvedValue([]);
  });

  it("opens an in-panel inspector for hosts and proxies with quick actions", () => {
    const onSelectHost = vi.fn();
    const onNavigateModule = vi.fn();
    const onOpenTerminal = vi.fn();
    const onOpenExecute = vi.fn();

    render(
      <I18nProvider>
        <ConnectionTopology
          hosts={[
            {
              name: "prod-api",
              host: "10.0.1.5",
              user: "deploy",
              port: 22,
              group: "prod",
              proxy_id: "corp-socks",
            },
          ]}
          proxies={[
            {
              id: "corp-socks",
              name: "Corp SOCKS",
              protocol: "socks5",
              host: "127.0.0.1",
              port: 1080,
            },
          ]}
          connectionStatuses={[
            { host: "prod-api", connected: true, healthy: true, reconnecting: false },
          ]}
          daemonHealth={{ ok: true, version: "0.5.0" }}
          onSelectHost={onSelectHost}
          onNavigateModule={onNavigateModule}
          onOpenTerminal={onOpenTerminal}
          onOpenExecute={onOpenExecute}
        />
      </I18nProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "prod-api" }));
    expect(onNavigateModule).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Node details")).toBeTruthy();
    expect(screen.getByText("deploy@10.0.1.5:22")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    expect(onOpenTerminal).toHaveBeenCalledWith("prod-api");

    fireEvent.click(screen.getByRole("button", { name: "Execute" }));
    expect(onOpenExecute).toHaveBeenCalledWith("prod-api");

    // Clicking the proxy link inside the host inspector switches to the proxy node.
    const inspector = screen.getByLabelText("Node details");
    fireEvent.click(within(inspector).getByRole("button", { name: /Corp SOCKS/ }));
    expect(screen.getByText("SOCKS5 · 127.0.0.1:1080")).toBeTruthy();
  });
});
