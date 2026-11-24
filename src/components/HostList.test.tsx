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
    onOpenAddHost: vi.fn(),
    onRemove: vi.fn(),
    onBatchRemove: vi.fn(),
    onRefresh: vi.fn(),
    onConnect: vi.fn(),
    onDisconnect: vi.fn(),
    onImportOpensshTrust: vi.fn(),
    onForgetOpenssh: vi.fn(),
    onImportNetcatty: vi.fn(),
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
    const { props } = renderHostList({ hosts: [] });

    expect(screen.getByText("No hosts configured")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Import from Netcatty" }));
    expect(props.onImportNetcatty).toHaveBeenCalledTimes(1);
  });

  it("keeps the Netcatty import off the list header once hosts exist", () => {
    renderHostList();

    expect(screen.queryByRole("button", { name: "Import from Netcatty" })).toBeNull();
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

  it("forgets a host in the system OpenSSH known_hosts from the row action", () => {
    const { props } = renderHostList();

    fireEvent.click(screen.getByTitle("More actions for web-01"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Forget web-01 in OpenSSH known_hosts" }));

    expect(props.onForgetOpenssh).toHaveBeenCalledWith(expect.objectContaining({ name: "web-01" }));
  });

  it("imports OpenSSH trust from the header action", () => {
    const { props } = renderHostList();

    fireEvent.click(screen.getByTitle("Import trust from OpenSSH (~/.ssh/known_hosts)"));

    expect(props.onImportOpensshTrust).toHaveBeenCalledTimes(1);
  });

  it("filters hosts by env", () => {
    renderHostList({
      hosts: [
        makeHost({ name: "web-01", env: "prod" }),
        makeHost({ name: "web-02", host: "10.0.0.2", env: "staging" }),
      ],
    });

    expect(screen.queryByPlaceholderText("env")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Attribute filters/ }));
    fireEvent.change(screen.getByPlaceholderText("env"), { target: { value: "prod" } });

    expect(screen.getByText("web-01")).toBeTruthy();
    expect(screen.queryByText("web-02")).toBeNull();
    expect(screen.getByText("1 of 2 hosts")).toBeTruthy();
  });

  it("opens add host from the toolbar and still edits from the row", () => {
    const { props } = renderHostList();

    fireEvent.click(screen.getByRole("button", { name: "Add Host" }));
    expect(props.onOpenAddHost).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByTitle("Edit web-01"));
    expect(props.onEdit).toHaveBeenCalledWith(expect.objectContaining({ name: "web-01" }));
  });

  it("highlights and scrolls the saved host into view", () => {
    const scroll = vi.fn();
    HTMLElement.prototype.scrollIntoView = scroll;
    renderHostList({
      hosts: [makeHost({ name: "web-01" }), makeHost({ name: "web-02", host: "10.0.0.2" })],
      highlightHost: "web-02",
    });
    const row = screen.getByText("web-02").closest("tr")!;
    expect(row.getAttribute("data-host")).toBe("web-02");
    expect(row.className).toContain("ring-primary");
    expect(scroll).toHaveBeenCalled();
  });

  it("shows connection progress only on the matching host row", () => {
    renderHostList({
      hosts: [makeHost({ name: "web-01" }), makeHost({ name: "web-02", host: "10.0.0.2" })],
      connectionProgress: {
        host: "web-01",
        message: "Opening SSH connection...",
        percent: 60,
        busy: true,
      },
    });

    const row = screen.getByText("web-01").closest("tr")!;
    expect(within(row).getByRole("progressbar").getAttribute("aria-valuenow")).toBe("60");
    expect(within(row).getByText("Opening SSH connection...")).toBeTruthy();
    expect(within(row).getByText("60%")).toBeTruthy();

    const other = screen.getByText("web-02").closest("tr")!;
    expect(within(other).queryByRole("progressbar")).toBeNull();
  });

  it("removes a host after confirming in the dialog", () => {
    const { props } = renderHostList();

    fireEvent.click(screen.getByTitle("More actions for web-01"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Remove web-01" }));
    expect(screen.getByText("Remove host web-01?")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Remove" }));

    expect(props.onRemove).toHaveBeenCalledWith("web-01");
  });

  it("shows every host from the All group and counts the full list", () => {
    const { props, rerender } = renderHostList({
      selectedGroup: "__all__",
      groups: [
        { id: "default", name: "Default", color: "#4A6CF7", sort_order: 0 },
        { id: "prod", name: "Prod", color: "#22c55e", sort_order: 1 },
      ],
      hosts: [
        makeHost({ name: "web-01", group: "default" }),
        makeHost({ name: "db-01", host: "10.0.0.9", user: "root", group: "prod" }),
      ],
    });

    expect(screen.getByText("web-01")).toBeTruthy();
    expect(screen.getByText("db-01")).toBeTruthy();
    const all = screen.getByRole("button", { name: /All hosts/ });
    expect(within(all).getByText("2")).toBeTruthy();
    const defaultGroup = screen.getByRole("button", { name: /Default/ });
    expect(
      all.compareDocumentPosition(defaultGroup) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();

    fireEvent.click(all);
    expect(props.onGroupSelect).toHaveBeenCalledWith("__all__");

    rerender(
      <I18nProvider>
        <HostList {...props} selectedGroup="prod" />
      </I18nProvider>
    );
    expect(screen.queryByText("web-01")).toBeNull();
    expect(screen.getByText("db-01")).toBeTruthy();
    expect(within(screen.getByRole("button", { name: /All hosts/ })).getByText("2")).toBeTruthy();
  });

  it("filters by a case-insensitive substring and clears the search", () => {
    renderHostList({
      hosts: [
        makeHost({ name: "web-01", host: "10.1.2.3", user: "deploy", tags: ["edge"] }),
        makeHost({ name: "db-01", host: "10.9.9.9", user: "root", tags: ["data"] }),
      ],
    });

    const search = screen.getByPlaceholderText("Search hosts, addresses, tags");
    fireEvent.change(search, { target: { value: "10.1" } });
    expect(screen.getByText("web-01")).toBeTruthy();
    expect(screen.queryByText("db-01")).toBeNull();

    fireEvent.change(search, { target: { value: "DATA" } });
    expect(screen.queryByText("web-01")).toBeNull();
    expect(screen.getByText("db-01")).toBeTruthy();

    fireEvent.change(search, { target: { value: "ROOT" } });
    expect(screen.getByText("db-01")).toBeTruthy();
    expect(screen.queryByText("web-01")).toBeNull();

    fireEvent.click(screen.getByTitle("Clear search"));
    expect(screen.getByText("web-01")).toBeTruthy();
    expect(screen.getByText("db-01")).toBeTruthy();
  });

  it("matches attribute filters by case-insensitive substring and shows the active count", () => {
    renderHostList({
      hosts: [
        makeHost({ name: "web-01", env: "Production", role: "edge", owner: "SRE", tags: ["edge-tier"] }),
        makeHost({ name: "web-02", host: "10.0.0.2", env: "staging", role: "db", owner: "Data", tags: ["core"] }),
      ],
    });

    expect(screen.queryByPlaceholderText("env")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Attribute filters/ }));
    fireEvent.change(screen.getByPlaceholderText("env"), { target: { value: "prod" } });
    expect(screen.getByText("web-01")).toBeTruthy();
    expect(screen.queryByText("web-02")).toBeNull();
    expect(within(screen.getByRole("button", { name: /Attribute filters/ })).getByText("1")).toBeTruthy();

    fireEvent.change(screen.getByPlaceholderText("env"), { target: { value: "" } });
    fireEvent.change(screen.getByPlaceholderText("tag"), { target: { value: "EDGE" } });
    expect(screen.getByText("web-01")).toBeTruthy();
    expect(screen.queryByText("web-02")).toBeNull();

    fireEvent.change(screen.getByPlaceholderText("tag"), { target: { value: "" } });
    fireEvent.change(screen.getByPlaceholderText("owner"), { target: { value: "sr" } });
    expect(screen.getByText("web-01")).toBeTruthy();
    expect(screen.queryByText("web-02")).toBeNull();
  });

  it("opens terminal, execute and SFTP from the row without selecting the host", () => {
    const { props } = renderHostList({
      onOpenTerminal: vi.fn(),
      onOpenExecute: vi.fn(),
      onOpenFiles: vi.fn(),
    });

    const row = screen.getByText("web-01").closest("tr")!;
    fireEvent.click(within(row).getByTitle("Open terminal web-01"));
    fireEvent.click(within(row).getByTitle("Execute on web-01"));
    fireEvent.click(within(row).getByTitle("Open SFTP web-01"));
    fireEvent.click(within(row).getByTitle("More actions for web-01"));

    expect(props.onOpenTerminal).toHaveBeenCalledWith("web-01");
    expect(props.onOpenExecute).toHaveBeenCalledWith("web-01");
    expect(props.onOpenFiles).toHaveBeenCalledWith("web-01");
    expect(props.onSelect).not.toHaveBeenCalled();
  });
});
