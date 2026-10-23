import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { NetcattyImportReport } from "../types";
import NetcattyImportDialog from "./NetcattyImportDialog";
import { ToastProvider } from "./ui/toast";

// Stub the API and the file picker: no test may reach Tauri, the filesystem or
// a real Netcatty backup.
const mockApi = vi.hoisted(() => ({
  setTrayLabels: vi.fn().mockResolvedValue(undefined),
  netcattyImport: vi.fn(),
  openDialog: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mockApi.openDialog }));
vi.mock("../api", () => ({ api: mockApi, reportError: vi.fn() }));

function makeReport(overrides: Partial<NetcattyImportReport> = {}): NetcattyImportReport {
  return {
    dryRun: false,
    hostsPlanned: 32,
    hostsImported: ["web-01", "db-01"],
    hostsSkippedExisting: [],
    hostsRenamed: [],
    keysWritten: ["id_ed25519"],
    keysSkippedExisting: [],
    passwordsSealed: 20,
    groupsCreated: ["AWS/Production", "homedata"],
    snippetsImported: [],
    snippetsSkippedExisting: [],
    jumpHostsLinked: 5,
    warnings: [],
    forwardsNotImported: [],
    ...overrides,
  };
}

function renderDialog(
  props: Partial<React.ComponentProps<typeof NetcattyImportDialog>> = {}
) {
  const onClose = vi.fn();
  const onImported = vi.fn();
  const view = render(
    <I18nProvider>
      <ToastProvider>
        <NetcattyImportDialog open onClose={onClose} onImported={onImported} {...props} />
      </ToastProvider>
    </I18nProvider>
  );
  return { ...view, onClose, onImported };
}

describe("NetcattyImportDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("starts on the file picker and asks the backend for a dry run", async () => {
    mockApi.openDialog.mockResolvedValue("/tmp/backup.json");
    mockApi.netcattyImport.mockResolvedValue(makeReport({ dryRun: true }));
    renderDialog();

    fireEvent.click(screen.getByRole("button", { name: "Choose backup file…" }));

    await waitFor(() =>
      expect(mockApi.netcattyImport).toHaveBeenCalledWith("/tmp/backup.json", {
        dryRun: true,
        skipCredentials: false,
      })
    );
    // The preview shows the plan, not the result: nothing has been written yet.
    expect(await screen.findByText(/2 host\(s\)/)).toBeTruthy();
  });

  it("imports for real only after the user confirms the preview", async () => {
    mockApi.openDialog.mockResolvedValue("/tmp/backup.json");
    mockApi.netcattyImport
      .mockResolvedValueOnce(makeReport({ dryRun: true }))
      .mockResolvedValueOnce(makeReport({ dryRun: false }));
    const { onImported } = renderDialog();

    fireEvent.click(screen.getByRole("button", { name: "Choose backup file…" }));
    await screen.findByText(/2 host\(s\)/);

    // No write happened during the preview.
    expect(mockApi.netcattyImport).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Import" }));

    await waitFor(() =>
      expect(mockApi.netcattyImport).toHaveBeenLastCalledWith("/tmp/backup.json", {
        dryRun: false,
        skipCredentials: false,
      })
    );
    await waitFor(() => expect(onImported).toHaveBeenCalled());
    expect(await screen.findByText("Done")).toBeTruthy();
  });

  it("passes the skip-credentials choice through to both calls", async () => {
    mockApi.openDialog.mockResolvedValue("/tmp/backup.json");
    mockApi.netcattyImport
      .mockResolvedValueOnce(makeReport({ dryRun: true }))
      .mockResolvedValueOnce(makeReport({ dryRun: false, passwordsSealed: 0 }));
    renderDialog();

    fireEvent.click(screen.getByRole("button", { name: "Choose backup file…" }));
    await screen.findByText(/2 host\(s\)/);

    fireEvent.click(
      screen.getByLabelText("Import hosts only — leave passwords and private keys behind")
    );
    fireEvent.click(screen.getByRole("button", { name: "Import" }));

    await waitFor(() =>
      expect(mockApi.netcattyImport).toHaveBeenLastCalledWith("/tmp/backup.json", {
        dryRun: false,
        skipCredentials: true,
      })
    );
  });

  it("surfaces a locked-store refusal instead of pretending the import worked", async () => {
    mockApi.openDialog.mockResolvedValue("/tmp/backup.json");
    mockApi.netcattyImport
      .mockResolvedValueOnce(makeReport({ dryRun: true }))
      .mockRejectedValueOnce(
        "20 host(s) in this backup carry passwords, but the credential store is locked"
      );
    const { onImported } = renderDialog();

    fireEvent.click(screen.getByRole("button", { name: "Choose backup file…" }));
    await screen.findByText(/2 host\(s\)/);
    fireEvent.click(screen.getByRole("button", { name: "Import" }));

    // Back to the preview with the error shown, and the caller is not told the
    // import succeeded.
    expect(await screen.findByText(/credential store is locked/)).toBeTruthy();
    expect(onImported).not.toHaveBeenCalled();
  });
});
