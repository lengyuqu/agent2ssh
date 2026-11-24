import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { type ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { HostProfile, SshKeyInfo } from "../types";
import KeysPanel, { keyMatchesHost } from "./KeysPanel";
import { ConfirmHost } from "./ui/dialog";
import { ToastProvider } from "./ui/toast";

// The panel lists keys on mount and asks the backend which algorithm this build
// can authenticate with. Stub the whole api module so no test reaches Tauri.
const mockApi = vi.hoisted(() => ({
  // I18nProvider pushes tray labels on mount.
  setTrayLabels: vi.fn().mockResolvedValue(undefined),
  listKeys: vi.fn(),
  defaultKeyAlgorithm: vi.fn(),
  generateKey: vi.fn(),
  deleteKey: vi.fn(),
}));

vi.mock("../api", () => ({
  api: mockApi,
  reportError: vi.fn(),
}));

function renderPanel(props: Partial<ComponentProps<typeof KeysPanel>> = {}) {
  return render(
    <I18nProvider>
      <ToastProvider>
        <ConfirmHost />
        <KeysPanel {...props} />
      </ToastProvider>
    </I18nProvider>
  );
}

function keyInfo(privatePath: string, name = "id-work"): SshKeyInfo {
  return {
    name,
    private_path: privatePath,
    public_path: `${privatePath}.pub`,
    public_key: "ssh-ed25519 AAAA",
    key_type: "ed25519",
  };
}

function hostUsing(name: string, keyPath?: string | null): HostProfile {
  return { name, host: "10.0.0.1", group: "default", key_path: keyPath };
}

/** Open the "Add Key" form and type a name. */
async function openFormWithName(name: string) {
  fireEvent.click(screen.getByText("Add Key"));
  fireEvent.change(screen.getByPlaceholderText("Key name (e.g. id-work)"), {
    target: { value: name },
  });
}

describe("KeysPanel key algorithm", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockApi.listKeys.mockResolvedValue([]);
    mockApi.defaultKeyAlgorithm.mockResolvedValue("ed25519");
    mockApi.generateKey.mockResolvedValue({
      name: "id-work",
      private_path: "/tmp/id-work",
      public_path: "/tmp/id-work.pub",
      public_key: "ssh-ed25519 AAAA",
      key_type: "ed25519",
    });
    mockApi.deleteKey.mockResolvedValue(undefined);
  });

  it("marks the backend's recommended algorithm and defers to it by default", async () => {
    mockApi.defaultKeyAlgorithm.mockResolvedValue("rsa");
    renderPanel();

    await waitFor(() => expect(mockApi.defaultKeyAlgorithm).toHaveBeenCalled());
    await openFormWithName("id-work");

    // The recommendation is labelled, and generating without choosing sends no
    // algorithm at all so the backend's own default applies — the form and the
    // backend can therefore never disagree about which key was written.
    expect(screen.getByText("recommended")).toBeTruthy();
    fireEvent.click(screen.getByText("Generate Key"));

    await waitFor(() =>
      expect(mockApi.generateKey).toHaveBeenCalledWith("id-work", undefined, undefined)
    );
  });

  it("lets the user override the recommendation", async () => {
    renderPanel();
    await waitFor(() => expect(mockApi.defaultKeyAlgorithm).toHaveBeenCalled());

    await openFormWithName("id-work");
    fireEvent.click(screen.getByText("RSA (traditional PEM)"));
    fireEvent.click(screen.getByText("Generate Key"));

    await waitFor(() =>
      expect(mockApi.generateKey).toHaveBeenCalledWith("id-work", undefined, "rsa")
    );
  });

  it("honours an explicit Ed25519 choice even when RSA is recommended", async () => {
    mockApi.defaultKeyAlgorithm.mockResolvedValue("rsa");
    renderPanel();
    await waitFor(() => expect(mockApi.defaultKeyAlgorithm).toHaveBeenCalled());

    await openFormWithName("id-work");
    fireEvent.click(screen.getByText("Ed25519"));
    fireEvent.click(screen.getByText("Generate Key"));

    await waitFor(() =>
      expect(mockApi.generateKey).toHaveBeenCalledWith("id-work", undefined, "ed25519")
    );
  });

  // The "this build cannot read Ed25519" warning is gone: `default_for_build()`
  // (`src-tauri/src/keys.rs:190`) returns `Ed25519` unconditionally, so the
  // `preferredAlgorithm === "rsa"` branch it hung off was unreachable against a
  // real backend, and the transport reads Ed25519 either way. L2-58.
  it("offers Ed25519 as the recommendation and never warns about it", async () => {
    renderPanel();
    await waitFor(() => expect(mockApi.defaultKeyAlgorithm).toHaveBeenCalled());

    await openFormWithName("id-work");
    fireEvent.click(screen.getByText("Ed25519"));

    expect(screen.queryByText(/cannot read Ed25519 keys/)).toBeNull();
  });

  // An older backend has no `default_key_algorithm` command, and the bridge can
  // be missing entirely in tests. Neither may break the form, and since we then
  // do not know what the build can use, no algorithm is sent at all and the
  // backend picks.
  it("sends no algorithm when the probe is unavailable", async () => {
    mockApi.defaultKeyAlgorithm.mockRejectedValue(new Error("command not found"));
    renderPanel();

    await openFormWithName("id-work");
    fireEvent.click(screen.getByText("Generate Key"));

    await waitFor(() =>
      expect(mockApi.generateKey).toHaveBeenCalledWith("id-work", undefined, undefined)
    );
  });
});

describe("keyMatchesHost", () => {
  const managed = keyInfo("/Users/yuqu/.agent2ssh/keys/id-work");

  it("treats a tilde path, an absolute path, and either slash as the same file", () => {
    expect(keyMatchesHost(managed, hostUsing("abs", "/Users/yuqu/.agent2ssh/keys/id-work"))).toBe(
      true
    );
    expect(keyMatchesHost(managed, hostUsing("tilde", "~/.agent2ssh/keys/id-work"))).toBe(true);
    expect(
      keyMatchesHost(managed, hostUsing("win", "\\Users\\yuqu\\.agent2ssh\\keys\\id-work"))
    ).toBe(true);
    expect(
      keyMatchesHost(managed, hostUsing("slash", "/Users/yuqu//.agent2ssh/keys/id-work/"))
    ).toBe(true);
    expect(
      keyMatchesHost(
        keyInfo("C:\\Users\\yuqu\\.ssh\\id_rsa", "id-rsa"),
        hostUsing("drive", "~/.ssh/id_rsa")
      )
    ).toBe(true);
    expect(
      keyMatchesHost(keyInfo("~/.ssh/id_ed25519"), hostUsing("home", "/home/yuqu/.ssh/id_ed25519"))
    ).toBe(true);
    expect(
      keyMatchesHost(keyInfo("~/.ssh/id_ed25519"), hostUsing("root", "/root/.ssh/id_ed25519"))
    ).toBe(true);
  });

  it("does not match a different file, a non-home prefix, or an empty path", () => {
    expect(keyMatchesHost(managed, hostUsing("other", "/Users/yuqu/.agent2ssh/keys/other"))).toBe(
      false
    );
    expect(keyMatchesHost(managed, hostUsing("opt", "/opt/keys/id-work"))).toBe(false);
    expect(keyMatchesHost(managed, hostUsing("user", "~other/.agent2ssh/keys/id-work"))).toBe(
      false
    );
    expect(keyMatchesHost(managed, hostUsing("bare"))).toBe(false);
    expect(keyMatchesHost(managed, hostUsing("blank", "   "))).toBe(false);
    expect(keyMatchesHost(managed, hostUsing("linux", "/home/yuqu/.agent2ssh/keys/id-work"))).toBe(
      false
    );
  });
});

describe("KeysPanel host chips", () => {
  const managed = keyInfo("/Users/yuqu/.agent2ssh/keys/id-work");
  const unused = keyInfo("/Users/yuqu/.agent2ssh/keys/unused", "unused");
  const hosts = [
    hostUsing("web-01", "~/.agent2ssh/keys/id-work"),
    hostUsing("win-01", "\\Users\\yuqu\\.agent2ssh\\keys\\id-work"),
    hostUsing("db-01", "/etc/keys/other"),
  ];

  beforeEach(() => {
    mockApi.listKeys.mockResolvedValue([managed, unused]);
    mockApi.deleteKey.mockReset();
    mockApi.deleteKey.mockResolvedValue(undefined);
  });

  it("shows a chip for each host that uses the key and selects it", async () => {
    const onSelectHost = vi.fn();
    renderPanel({ hosts, onSelectHost });

    expect(await screen.findByRole("button", { name: "web-01" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "win-01" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "db-01" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "web-01" }));
    expect(onSelectHost).toHaveBeenCalledWith("web-01");
  });

  it("warns with the host list before deleting a key that is in use", async () => {
    renderPanel({ hosts });
    fireEvent.click(await screen.findByTitle("Delete id-work"));

    expect(
      await screen.findByText("These hosts use this key: web-01, win-01")
    ).toBeTruthy();
    expect(screen.queryByText(/db-01/)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /^Delete$/ }));
    await waitFor(() => expect(mockApi.deleteKey).toHaveBeenCalledWith("id-work"));
  });

  it("does not warn when no host uses the key", async () => {
    renderPanel({ hosts });
    fireEvent.click(await screen.findByTitle("Delete unused"));

    expect(await screen.findByText('Delete key "unused"?')).toBeTruthy();
    expect(screen.queryByText(/These hosts use this key/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(mockApi.deleteKey).not.toHaveBeenCalled();
  });
});
