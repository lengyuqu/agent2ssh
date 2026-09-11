import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { AlgoPrefsState } from "../types";
import AlgoPrefsDialog from "./AlgoPrefsDialog";

const mockApi = vi.hoisted(() => ({
  // I18nProvider pushes tray labels on mount/language change.
  setTrayLabels: vi.fn().mockResolvedValue(undefined),
  getAlgoPrefs: vi.fn(),
  setAlgoPrefs: vi.fn(),
  clearAlgoPrefs: vi.fn(),
}));

vi.mock("../api", () => ({
  api: mockApi,
  reportError: vi.fn(),
}));

const DEFAULTS = {
  kex: "curve25519-sha256",
  hostkey: "ssh-ed25519",
  cipher_cs: "aes256-ctr",
  cipher_sc: "aes256-ctr",
  mac_cs: "hmac-sha2-256",
  mac_sc: "hmac-sha2-256",
  comp_cs: "none",
  comp_sc: "none",
};

function state(overrides: Partial<AlgoPrefsState> = {}): AlgoPrefsState {
  return { prefs: { ...DEFAULTS }, custom: false, defaults: { ...DEFAULTS }, ...overrides };
}

function renderDialog() {
  const onClose = vi.fn();
  const view = render(
    <I18nProvider>
      <AlgoPrefsDialog onClose={onClose} />
    </I18nProvider>
  );
  return { ...view, onClose };
}

describe("AlgoPrefsDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockApi.getAlgoPrefs.mockResolvedValue(state());
    mockApi.setAlgoPrefs.mockResolvedValue(undefined);
    mockApi.clearAlgoPrefs.mockResolvedValue(undefined);
  });

  it("reports the defaults state and keeps Save disabled until edited", async () => {
    renderDialog();

    expect(await screen.findByText("Built-in defaults")).toBeTruthy();
    const save = screen.getByRole("button", { name: "Save" });
    expect((save as HTMLButtonElement).disabled).toBe(true);

    fireEvent.change(screen.getByLabelText("Key exchange"), {
      target: { value: "curve25519-sha256,curve25519-sha256@libssh.org" },
    });

    expect((save as HTMLButtonElement).disabled).toBe(false);
    // The field now diverges from `defaults`, so the badge appears.
    expect(screen.getAllByText("Differs from defaults").length).toBe(1);
  });

  it("persists the edited preferences through the backend command", async () => {
    // First read is the pristine defaults; the post-save re-read reports the
    // user's override, which is what flips the badge to "Custom".
    mockApi.getAlgoPrefs
      .mockResolvedValueOnce(state())
      .mockResolvedValue(
        state({ custom: true, prefs: { ...DEFAULTS, hostkey: "ssh-ed25519,rsa-sha2-512" } })
      );
    renderDialog();
    await screen.findByText("Built-in defaults");

    fireEvent.change(screen.getByLabelText("Host key"), {
      target: { value: "ssh-ed25519,rsa-sha2-512" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockApi.setAlgoPrefs).toHaveBeenCalledTimes(1));
    expect(mockApi.setAlgoPrefs).toHaveBeenCalledWith({
      ...DEFAULTS,
      hostkey: "ssh-ed25519,rsa-sha2-512",
    });
    // After a successful save the dialog re-reads state, so it should now be
    // flagged custom. The badge tracks divergence from the *built-in defaults*
    // (not from the saved value), so the overridden field keeps it.
    expect(await screen.findByText("Custom")).toBeTruthy();
    expect(screen.getAllByText("Differs from defaults").length).toBe(1);
  });

  it("surfaces a validator rejection instead of closing", async () => {
    mockApi.setAlgoPrefs.mockRejectedValue("comp_cs: only 'none' is supported");
    renderDialog();
    await screen.findByText("Built-in defaults");

    fireEvent.change(screen.getByLabelText("Compression (client → server)"), {
      target: { value: "zlib" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText("comp_cs: only 'none' is supported")).toBeTruthy();
  });

  it("reverts to the built-in defaults via clear", async () => {
    mockApi.getAlgoPrefs
      .mockResolvedValueOnce(state({ custom: true, prefs: { ...DEFAULTS, kex: "diffie-hellman-group14-sha256" } }))
      .mockResolvedValue(state());
    renderDialog();

    expect(await screen.findByText("Custom")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Reset to defaults" }));

    await waitFor(() => expect(mockApi.clearAlgoPrefs).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("Built-in defaults")).toBeTruthy();
  });
});
