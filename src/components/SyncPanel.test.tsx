import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { WebDavSyncConfig, WebDavSyncStatus } from "../types";
import SyncPanel from "./SyncPanel";
import { ToastProvider } from "./ui/toast";

// Stub the whole API, including I18nProvider's tray update, so no test invokes
// Tauri or makes a request to a real WebDAV server.
const mockApi = vi.hoisted(() => ({
  setTrayLabels: vi.fn(),
  getWebDavSyncConfig: vi.fn(),
  getWebDavSyncStatus: vi.fn(),
  setWebDavSyncConfig: vi.fn(),
  testWebDavSync: vi.fn(),
  pushWebDavSync: vi.fn(),
  downloadWebDavSync: vi.fn(),
  restoreWebDavSync: vi.fn(),
  saveDialog: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: mockApi.saveDialog }));

vi.mock("../api", () => ({ api: mockApi, reportError: vi.fn() }));

const configured: WebDavSyncConfig = {
  enabled: true,
  url: "https://example.com/dav",
  username: "backup-user",
  remotePath: "agent2ssh/agent2ssh-sync.json",
  passwordConfigured: true,
  syncPasswordConfigured: true,
};

const readyStatus: WebDavSyncStatus = {
  configured: true,
  enabled: true,
  syncState: "unknown",
  syncSummary: "No upload yet.",
};

async function renderPanel() {
  const view = render(
    <I18nProvider>
      <ToastProvider>
        <SyncPanel />
      </ToastProvider>
    </I18nProvider>
  );
  // Wait for both initial reads to finish; otherwise a disabled upload could
  // mean only that the loading state is still active.
  await waitFor(() => {
    expect((screen.getByLabelText("WebDAV URL") as HTMLInputElement).value).toBe(
      configured.url
    );
    expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(false);
  });
  return view;
}

function encryptionPassword(): HTMLInputElement {
  return screen.getByLabelText(/^Sync encryption password/) as HTMLInputElement;
}

function uploadButton(): HTMLButtonElement {
  return screen.getByRole("button", { name: "Upload now" }) as HTMLButtonElement;
}

describe("SyncPanel encrypted upload settings", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    localStorage.setItem("agent2ssh.language", "en");
    mockApi.setTrayLabels.mockResolvedValue(undefined);
    mockApi.getWebDavSyncConfig.mockResolvedValue(configured);
    mockApi.getWebDavSyncStatus.mockResolvedValue(readyStatus);
    mockApi.setWebDavSyncConfig.mockResolvedValue(configured);
    mockApi.testWebDavSync.mockResolvedValue({ ...readyStatus, lastSuccess: true });
    mockApi.pushWebDavSync.mockResolvedValue({
      ...readyStatus,
      lastSuccess: true,
      lastMessage: "Encrypted upload completed.",
    });
  });

  it.each([false, undefined])(
    "blocks an enabled legacy configuration without an encryption password (%s)",
    async (syncPasswordConfigured) => {
      mockApi.getWebDavSyncConfig.mockResolvedValue({ ...configured, syncPasswordConfigured });
      await renderPanel();

      expect(uploadButton().disabled).toBe(true);
      fireEvent.click(uploadButton());
      expect(mockApi.pushWebDavSync).not.toHaveBeenCalled();

      // Typing alone is insufficient: the upload uses the saved vault secret.
      fireEvent.change(encryptionPassword(), { target: { value: "unsaved-sync-password" } });
      expect(uploadButton().disabled).toBe(true);
      expect(mockApi.setWebDavSyncConfig).not.toHaveBeenCalled();
    }
  );

  it("uploads with an already configured password while leaving the password field empty", async () => {
    await renderPanel();

    expect(encryptionPassword().type).toBe("password");
    expect(encryptionPassword().value).toBe("");
    expect(encryptionPassword().placeholder).toBe("Saved. Leave blank to keep it.");
    expect(uploadButton().disabled).toBe(false);
    fireEvent.click(uploadButton());

    await waitFor(() => expect(mockApi.pushWebDavSync).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("Encrypted upload completed.", { selector: "div.mt-1" })).toBeTruthy();
    expect(mockApi.setWebDavSyncConfig).not.toHaveBeenCalled();
  });

  it("sends the new password only in the save request and clears it after saving", async () => {
    mockApi.getWebDavSyncConfig.mockResolvedValue({ ...configured, syncPasswordConfigured: false });
    const { container } = await renderPanel();
    const secret = "  test-encryption-password  ";
    fireEvent.change(encryptionPassword(), { target: { value: secret } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(mockApi.setWebDavSyncConfig).toHaveBeenCalledWith({
        enabled: true,
        url: configured.url,
        username: configured.username,
        password: null,
        syncPassword: secret,
        remotePath: configured.remotePath,
      })
    );
    await waitFor(() => {
      expect(encryptionPassword().value).toBe("");
      expect(uploadButton().disabled).toBe(false);
    });
    expect(screen.queryByDisplayValue(secret)).toBeNull();
    expect(container.textContent).not.toContain(secret);
    expect(encryptionPassword().placeholder).toBe("Saved. Leave blank to keep it.");
  });

  it("preserves saved passwords by sending null when password fields are left blank", async () => {
    await renderPanel();
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "renamed-user" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(mockApi.setWebDavSyncConfig).toHaveBeenCalledWith({
        enabled: true,
        url: configured.url,
        username: "renamed-user",
        password: null,
        syncPassword: null,
        remotePath: configured.remotePath,
      })
    );
    await waitFor(() => expect(uploadButton().disabled).toBe(false));
    expect(encryptionPassword().value).toBe("");
  });

  it("downloads with saved settings even when automatic upload is disabled", async () => {
    mockApi.getWebDavSyncConfig.mockResolvedValue({ ...configured, enabled: false });
    mockApi.saveDialog.mockResolvedValue("C:/backups/cloud.a2s");
    mockApi.downloadWebDavSync.mockResolvedValue({ fileCount: 1, localPath: "C:/backups/cloud.a2s" });
    await renderPanel();
    fireEvent.click(screen.getByRole("button", { name: "Download backup" }));
    await waitFor(() => expect(mockApi.downloadWebDavSync).toHaveBeenCalledWith("C:/backups/cloud.a2s"));
    expect(await screen.findByText("Encrypted backup downloaded: C:/backups/cloud.a2s", { selector: "p" })).toBeTruthy();
    expect(mockApi.restoreWebDavSync).not.toHaveBeenCalled();
  });

  it("cancelling the save dialog does not download or report success", async () => {
    mockApi.saveDialog.mockResolvedValue(null);
    await renderPanel();
    fireEvent.click(screen.getByRole("button", { name: "Download backup" }));
    await waitFor(() => expect(mockApi.saveDialog).toHaveBeenCalledTimes(1));
    await waitFor(() => expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(false));
    expect(mockApi.downloadWebDavSync).not.toHaveBeenCalled();
    expect(screen.queryByText(/Encrypted backup downloaded:/)).toBeNull();
  });

  it("requires confirmation before restoring and refreshes the app after success", async () => {
    mockApi.restoreWebDavSync.mockResolvedValue({ fileCount: 2, backupId: "undo-123" });
    const onRestored = vi.fn().mockResolvedValue(undefined);
    render(<I18nProvider><ToastProvider><SyncPanel onRestored={onRestored} /></ToastProvider></I18nProvider>);
    await waitFor(() => expect((screen.getByRole("button", { name: "Restore from cloud" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: "Restore from cloud" }));
    expect(mockApi.restoreWebDavSync).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(mockApi.restoreWebDavSync).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Restore from cloud" }));
    fireEvent.click(screen.getByRole("button", { name: /^Restore$/ }));
    await waitFor(() => expect(onRestored).toHaveBeenCalledTimes(1));
    expect(mockApi.restoreWebDavSync).toHaveBeenCalledTimes(1);
    expect(screen.getByText(/Cloud backup restored.*undo-123/, { selector: "p" })).toBeTruthy();
  });

  it("does not show success or refresh the app after a failed restore", async () => {
    mockApi.restoreWebDavSync.mockRejectedValue(new Error("wrong password or tampered backup"));
    await renderPanel();
    fireEvent.click(screen.getByRole("button", { name: "Restore from cloud" }));
    fireEvent.click(screen.getByRole("button", { name: /^Restore$/ }));
    expect(await screen.findByText(/wrong password or tampered backup/)).toBeTruthy();
    expect(screen.queryByText(/Cloud backup restored/)).toBeNull();
  });

  it("blocks recovery with missing password or unsaved settings", async () => {
    mockApi.getWebDavSyncConfig.mockResolvedValue({ ...configured, syncPasswordConfigured: false });
    const view = await renderPanel();
    expect((screen.getByRole("button", { name: "Restore from cloud" }) as HTMLButtonElement).disabled).toBe(true);
    view.unmount();
    mockApi.getWebDavSyncConfig.mockResolvedValue(configured);
    await renderPanel();
    fireEvent.change(screen.getByLabelText("WebDAV URL"), { target: { value: "https://unsaved.example" } });
    expect((screen.getByRole("button", { name: "Download backup" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: "Restore from cloud" }) as HTMLButtonElement).disabled).toBe(true);
  });
});
