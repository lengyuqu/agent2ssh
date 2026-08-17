import { fireEvent, render, screen, within } from "@testing-library/react";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";
import HostList from "./HostList";
import { I18nProvider } from "../i18n";
import type { ConnectionStatus, HostGroup, HostProfile, ProxyProfile } from "../types";

// HostList renders inside I18nProvider, which syncs tray labels through the
// Tauri bridge. Stub the api module so tests never touch Tauri internals.
vi.mock("../api", () => ({
  api: { setTrayLabels: vi.fn().mockResolvedValue(undefined) },
}));

const groups: HostGroup[] = [{ id: "default", name: "Default", color: "#4A6CF7", sort_order: 0 }];

function makeHost(overrides: Partial<HostProfile> = {}): HostProfile {
  return {
    name: "web-01",
    host: "10.0.0.1",
    user: "deploy",
    port: 22,
    group: "default",
    ...overrides,
  };
}

type HostListProps = ComponentProps<typeof HostList>;

function renderHostList(overrides: Partial<HostListProps> = {}) {
  const props: HostListProps = {
    hosts: [makeHost()],
    groups,
    proxies: [] as ProxyProfile[],
    selectedHost: "",
    selectedGroup: "default",
    connectionStatuses: [] as ConnectionStatus[],
    onSelect: vi.fn(),
    onGroupSelect: vi.fn(),
    onCreateGroup: vi.fn(),
    onRenameGroup: vi.fn(),
    onDeleteGroup: vi.fn(),
    onEdit: vi.fn(),
    onRemove: vi.fn(),
    onBatchRemove: vi.fn(),
    onRefresh: vi.fn(),
    onConnect: vi.fn(),
    onDisconnect: vi.fn(),
    ...overrides,
  };
  const view = render(
    <I18nProvider>
      <HostList {...props} />
    </I18nProvider>
  );
  return { ...view, props };
}

describe("HostList", () => {
  it("renders hosts with address and disconnected status", () => {
    renderHostList();

    expect(screen.getByText("web-01")).toBeTruthy();
    expect(screen.getByText("deploy@10.0.0.1:22")).toBeTruthy();
    expect(screen.getByText("Disconnected")).toBeTruthy();
    expect(screen.getByText("1 of 1 hosts")).toBeTruthy();
  });

  it("shows the empty state when no hosts are configured", () => {
    renderHostList({ hosts: [] });

    expect(screen.getByText("No hosts configured")).toBeTruthy();
  });

  it("selects a host when its row is clicked", () => {
    const { props } = renderHostList();

    fireEvent.click(screen.getByText("web-01"));

    expect(props.onSelect).toHaveBeenCalledWith("web-01");
  });

  it("connects a disconnected host from the row action", () => {
    const { props } = renderHostList();

    fireEvent.click(screen.getByTitle("Connect web-01"));

    expect(props.onConnect).toHaveBeenCalledWith("web-01");
  });

  it("disconnects a connected host from the row action", () => {
    const { props } = renderHostList({
      connectionStatuses: [{ host: "web-01", connected: true }],
    });

    const row = screen.getByText("web-01").closest<HTMLTableRowElement>("tr")!;
    expect(within(row).getByText("Connected")).toBeTruthy();
    fireEvent.click(within(row).getByTitle("Disconnect web-01"));

    expect(props.onDisconnect).toHaveBeenCalledWith("web-01");
  });

  it("filters hosts by env", () => {
    renderHostList({
      hosts: [
        makeHost({ name: "web-01", env: "prod" }),
        makeHost({ name: "web-02", host: "10.0.0.2", env: "staging" }),
      ],
    });

    fireEvent.change(screen.getByPlaceholderText("env"), { target: { value: "prod" } });

    expect(screen.getByText("web-01")).toBeTruthy();
    expect(screen.queryByText("web-02")).toBeNull();
    expect(screen.getByText("1 of 2 hosts")).toBeTruthy();
  });

  it("removes a host after confirming in the dialog", () => {
    const { props } = renderHostList();

    fireEvent.click(screen.getByTitle("Remove web-01"));
    expect(screen.getByText("Remove host web-01?")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Remove" }));

    expect(props.onRemove).toHaveBeenCalledWith("web-01");
  });
});
