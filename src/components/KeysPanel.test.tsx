import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import KeysPanel from "./KeysPanel";
import { ToastProvider } from "./ui/toast";

// The panel lists keys on mount and asks the backend which algorithm this build
// can authenticate with. Stub the whole api module so no test reaches Tauri.
const mockApi = vi.hoisted(() => ({
  // I18nProvider pushes tray labels on mount.
  setTrayLabels: vi.fn().mockResolvedValue(undefined),
  listKeys: vi.fn(),
  defaultKeyAlgorithm: vi.fn(),
  generateKey: vi.fn(),
}));

vi.mock("../api", () => ({
  api: mockApi,
  reportError: vi.fn(),
}));

function renderPanel() {
  return render(
    <I18nProvider>
      <ToastProvider>
        <KeysPanel />
      </ToastProvider>
    </I18nProvider>
  );
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

  it("warns before generating an Ed25519 key this build cannot read", async () => {
    mockApi.defaultKeyAlgorithm.mockResolvedValue("rsa");
    renderPanel();
    await waitFor(() => expect(mockApi.defaultKeyAlgorithm).toHaveBeenCalled());

    await openFormWithName("id-work");
    fireEvent.click(screen.getByText("Ed25519"));

    // The recommendation is RSA precisely because this build cannot read the
    // OpenSSH container Ed25519 keys are written in, so saying so is the point.
    expect(screen.getByText(/cannot read Ed25519 keys/)).toBeTruthy();
  });

  it("stays silent about the warning on a build that can read Ed25519", async () => {
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
