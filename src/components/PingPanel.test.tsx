import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HostProfile, PingResult } from "../types";
import PingPanel from "./PingPanel";
import { ToastProvider } from "./ui/toast";

const mockApi = vi.hoisted(() => ({
  pingHosts: vi.fn(),
}));

vi.mock("../api", () => ({
  api: mockApi,
  reportError: vi.fn(),
}));
vi.mock("../i18n", () => ({ useI18n: () => ({ t: (text: string) => text }) }));

function host(name: string): HostProfile {
  return { name, host: "10.0.0.8", user: "deploy", group: "default" };
}

const probed: PingResult[] = [
  { host: "snappy", reachable: true, latency_ms: 12 },
  { host: "lagging", reachable: true, latency_ms: 840 },
  { host: "down", reachable: false, error: "timed out" },
];

function hostNames() {
  return within(screen.getByRole("list"))
    .getAllByRole("listitem")
    .map((item) => item.querySelector("strong")?.textContent);
}

describe("PingPanel", () => {
  beforeEach(() => {
    mockApi.pingHosts.mockReset();
    mockApi.pingHosts.mockResolvedValue(probed);
  });

  it("sorts unreachable hosts first and fires the row actions", async () => {
    const onOpenTerminal = vi.fn();
    const onInspectHost = vi.fn();
    render(
      <ToastProvider>
        <PingPanel
          hosts={[host("snappy"), host("lagging"), host("down")]}
          onOpenTerminal={onOpenTerminal}
          onInspectHost={onInspectHost}
        />
      </ToastProvider>
    );

    fireEvent.click(screen.getByRole("button", { name: "Ping all hosts" }));
    await screen.findByRole("list");

    expect(hostNames()).toEqual(["down", "lagging", "snappy"]);

    const down = within(screen.getByRole("list")).getAllByRole("listitem")[0];
    fireEvent.click(within(down).getByRole("button", { name: /Open terminal/ }));
    fireEvent.click(within(down).getByRole("button", { name: /View host/ }));
    expect(onOpenTerminal).toHaveBeenCalledWith("down");
    expect(onInspectHost).toHaveBeenCalledWith("down");

    fireEvent.click(screen.getByRole("button", { name: "Problems only" }));
    expect(hostNames()).toEqual(["down"]);

    fireEvent.click(screen.getByRole("button", { name: "All" }));
    expect(hostNames()).toEqual(["down", "lagging", "snappy"]);
  });
});
