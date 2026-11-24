import { fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import TopBar from "./TopBar";

function renderTopBar(props: Partial<ComponentProps<typeof TopBar>> = {}) {
  const callbacks = {
    onOpenPalette: vi.fn(),
    onOpenApprovals: vi.fn(),
    onStartDaemon: vi.fn(),
    onGateToggle: vi.fn(),
    onRequestUnlock: vi.fn(),
    onNavigateModule: vi.fn(),
  };
  render(
    <I18nProvider>
      <TopBar
        daemonHealth={{ ok: true, version: "0.5.0" }}
        gateStatus={{ mode: "active", updated_at: "2026-11-24T00:00:00Z" }}
        secretsLocked={false}
        connectionStatuses={[
          { host: "web-1", connected: true, healthy: true, reconnecting: false },
        ]}
        pendingApprovalsCount={2}
        {...callbacks}
        {...props}
      />
    </I18nProvider>,
  );
  return callbacks;
}

describe("TopBar status pills", () => {
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem("agent2ssh.language", "en");
  });

  it("routes each live pill to its matching action", () => {
    const cb = renderTopBar();

    fireEvent.click(screen.getByRole("button", { name: /Daemon running/ }));
    expect(cb.onNavigateModule).toHaveBeenCalledWith("activity");

    fireEvent.click(screen.getByRole("button", { name: /Gate active/ }));
    expect(cb.onGateToggle).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: /Credentials unlocked/ }));
    expect(cb.onNavigateModule).toHaveBeenCalledWith("keys");

    fireEvent.click(screen.getByRole("button", { name: /1 active connections/ }));
    expect(cb.onNavigateModule).toHaveBeenCalledWith("hosts");

    fireEvent.click(screen.getByRole("button", { name: /2 pending approvals/ }));
    expect(cb.onOpenApprovals).toHaveBeenCalledWith();
  });

  it("starts the daemon when offline and prompts unlock when credentials are locked", () => {
    const cb = renderTopBar({
      daemonHealth: { ok: false },
      secretsLocked: true,
    });

    fireEvent.click(screen.getByRole("button", { name: /Daemon offline/ }));
    expect(cb.onStartDaemon).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: /Credentials locked/ }));
    expect(cb.onRequestUnlock).toHaveBeenCalledTimes(1);
    expect(cb.onNavigateModule).not.toHaveBeenCalled();
  });
});
