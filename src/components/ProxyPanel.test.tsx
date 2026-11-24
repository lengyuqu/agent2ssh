import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { HostProfile, ProxyProfile } from "../types";
import ProxyPanel from "./ProxyPanel";
import { ToastProvider } from "./ui/toast";

const mockApi = vi.hoisted(() => ({
  setTrayLabels: vi.fn().mockResolvedValue(undefined),
  saveProxy: vi.fn(),
  deleteProxy: vi.fn(),
}));

vi.mock("../api", () => ({
  api: mockApi,
  reportError: vi.fn(),
}));

function host(name: string, proxyId?: string | null): HostProfile {
  return { name, host: "10.0.0.1", group: "default", proxy_id: proxyId ?? null };
}

const office: ProxyProfile = {
  id: "office",
  name: "office",
  protocol: "http",
  host: "127.0.0.1",
  port: 8080,
};

const solo: ProxyProfile = {
  id: "solo",
  name: "solo",
  protocol: "socks5",
  host: "10.1.1.1",
  port: 1080,
};

const assigned = ["h1", "h2", "h3", "h4", "h5", "h6"].map((name) => host(name, "office"));

function renderPanel(onSelectHost = vi.fn()) {
  const view = render(
    <I18nProvider>
      <ToastProvider>
        <ProxyPanel
          proxies={[office, solo]}
          hosts={[...assigned, host("other", null)]}
          onChanged={vi.fn()}
          onSelectHost={onSelectHost}
        />
      </ToastProvider>
    </I18nProvider>
  );
  return { ...view, onSelectHost };
}

describe("ProxyPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockApi.saveProxy.mockResolvedValue(undefined);
    mockApi.deleteProxy.mockResolvedValue(undefined);
  });

  it("opens the add drawer and closes it after save", async () => {
    renderPanel();

    fireEvent.click(screen.getByRole("button", { name: "Add Proxy" }));
    expect(screen.getByRole("heading", { name: "Add Proxy" })).toBeTruthy();

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "lab" } });
    fireEvent.change(screen.getByLabelText("Proxy host"), { target: { value: "10.2.2.2" } });
    fireEvent.click(screen.getByRole("button", { name: "Save proxy" }));

    await waitFor(() => expect(mockApi.saveProxy).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(mockApi.saveProxy).toHaveBeenCalledWith(
      expect.objectContaining({ name: "lab", host: "10.2.2.2", protocol: "http", port: 8080 })
    );
  });

  it("edits a proxy from the drawer with the current values", () => {
    renderPanel();
    fireEvent.click(screen.getByTitle("Edit office"));
    expect(screen.getByRole("heading", { name: "Edit Proxy" })).toBeTruthy();
    expect((screen.getByLabelText("Name") as HTMLInputElement).value).toBe("office");
    expect((screen.getByLabelText("Proxy host") as HTMLInputElement).value).toBe("127.0.0.1");
  });

  it("shows five host chips, then the rest, and selects a host", () => {
    const { onSelectHost } = renderPanel();

    for (const name of ["h1", "h2", "h3", "h4", "h5"]) {
      expect(screen.getByRole("button", { name })).toBeTruthy();
    }
    expect(screen.queryByRole("button", { name: "h6" })).toBeNull();
    expect(screen.getByRole("button", { name: "+1" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "h1" }));
    expect(onSelectHost).toHaveBeenCalledWith("h1");

    fireEvent.click(screen.getByRole("button", { name: "+1" }));
    fireEvent.click(screen.getByRole("button", { name: "h6" }));
    expect(onSelectHost).toHaveBeenCalledWith("h6");
  });

  it("lists affected hosts in the delete confirm, and omits the list when none use it", () => {
    renderPanel();

    fireEvent.click(screen.getByTitle("Delete solo"));
    expect(screen.getByText("Delete proxy solo?")).toBeTruthy();
    expect(
      screen.getByText("Hosts using this proxy will switch back to direct connections.")
    ).toBeTruthy();
    expect(screen.queryByText(/Affected hosts/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    fireEvent.click(screen.getByTitle("Delete office"));
    expect(screen.getByText("Affected hosts: h1, h2, h3, h4, h5, h6")).toBeTruthy();
  });
});
