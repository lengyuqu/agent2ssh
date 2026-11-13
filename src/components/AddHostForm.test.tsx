import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { HostProfile } from "../types";
import AddHostForm from "./AddHostForm";
import { ToastProvider } from "./ui/toast";

const updateHost = vi.fn();
const addHost = vi.fn();

vi.mock("../api", () => ({
  api: {
    listKeys: vi.fn().mockResolvedValue([]),
    listHosts: vi.fn().mockResolvedValue([]),
    listHostGroups: vi.fn().mockResolvedValue([]),
    listProxies: vi.fn().mockResolvedValue([]),
    listDaemons: vi.fn().mockResolvedValue([]),
    saveHostGroup: vi.fn(),
    deleteHostGroup: vi.fn(),
    addHost: (host: HostProfile) => addHost(host),
    updateHost: (originalName: string, host: HostProfile) => updateHost(originalName, host),
    setTrayLabels: vi.fn().mockResolvedValue(undefined),
  },
  reportError: vi.fn(),
}));

function editingHost(overrides: Partial<HostProfile> = {}): HostProfile {
  return {
    name: "web-1",
    host: "10.0.0.5",
    user: "deploy",
    port: 22,
    key_path: null,
    password: "",
    jump_host: null,
    proxy_id: null,
    tags: [],
    group: "default",
    env: null,
    role: null,
    owner: null,
    ai_allowed: true,
    ...overrides,
  };
}

function renderForm(host: HostProfile | null, hosts: HostProfile[] = host ? [host] : []) {
  const onImportNetcatty = vi.fn();
  render(
    <I18nProvider>
      <ToastProvider>
        <AddHostForm
          hosts={hosts}
          groups={[{ id: "default", name: "default", color: "#4A6CF7", sort_order: 0 }]}
          proxies={[]}
          initialGroup="default"
          editingHost={host}
          onSaved={vi.fn().mockResolvedValue(undefined)}
          onCancelEdit={vi.fn()}
          onImportNetcatty={onImportNetcatty}
        />
      </ToastProvider>
    </I18nProvider>
  );
  return { onImportNetcatty };
}

function renderEditingForm(host: HostProfile) {
  render(
    <I18nProvider>
      <ToastProvider>
        <AddHostForm
          hosts={[host]}
          groups={[{ id: "default", name: "default", color: "#4A6CF7", sort_order: 0 }]}
          proxies={[]}
          initialGroup="default"
          editingHost={host}
          onSaved={vi.fn().mockResolvedValue(undefined)}
          onCancelEdit={vi.fn()}
        />
      </ToastProvider>
    </I18nProvider>
  );
}

describe("AddHostForm edit round trip", () => {
  beforeEach(() => {
    updateHost.mockClear();
    addHost.mockClear();
  });

  it("carries init_command through an edit it cannot show", async () => {
    // `update_host` replaces the stored profile wholesale; a payload that omits
    // this field turns the host's post-connect command off silently. The form
    // never renders it, so the only place to notice is here.
    const host = editingHost({ init_command: "cd /app && source env.sh" });
    renderEditingForm(host);

    fireEvent.submit(screen.getByText("Update host").closest("form")!);

    await waitFor(() => expect(updateHost).toHaveBeenCalledTimes(1));
    expect(updateHost.mock.calls[0][0]).toBe("web-1");
    expect(updateHost.mock.calls[0][1].init_command).toBe("cd /app && source env.sh");
  });

  it("sends null when the host never had one", async () => {
    renderForm(editingHost());

    fireEvent.submit(screen.getByText("Update host").closest("form")!);

    await waitFor(() => expect(updateHost).toHaveBeenCalledTimes(1));
    expect(updateHost.mock.calls[0][1].init_command).toBeNull();
  });

  it("offers the Netcatty import beside Add Host once a host exists", () => {
    const host = editingHost();
    const { onImportNetcatty } = renderForm(null, [host]);

    fireEvent.click(screen.getByRole("button", { name: "Import from Netcatty" }));
    expect(onImportNetcatty).toHaveBeenCalledTimes(1);
  });

  it("leaves the Netcatty import off an empty list and off an edit", () => {
    const { unmount } = render(
      <I18nProvider>
        <ToastProvider>
          <AddHostForm
            hosts={[]}
            groups={[{ id: "default", name: "default", color: "#4A6CF7", sort_order: 0 }]}
            proxies={[]}
            initialGroup="default"
            onSaved={vi.fn()}
            onImportNetcatty={vi.fn()}
          />
        </ToastProvider>
      </I18nProvider>
    );
    expect(screen.queryByRole("button", { name: "Import from Netcatty" })).toBeNull();
    unmount();

    renderEditingForm(editingHost());
    expect(screen.queryByRole("button", { name: "Import from Netcatty" })).toBeNull();
  });
});
