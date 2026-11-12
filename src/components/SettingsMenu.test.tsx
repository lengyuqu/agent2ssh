import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import SettingsMenu from "./SettingsMenu";
import { ConfirmHost } from "./ui/dialog";

const mockApi = vi.hoisted(() => ({
  getTelemetryEnabled: vi.fn().mockResolvedValue(false),
  secretsStatus: vi.fn(),
  secretsUnlock: vi.fn().mockResolvedValue(undefined),
  secretsChangePassword: vi.fn().mockResolvedValue(undefined),
  getCliPathStatus: vi.fn().mockResolvedValue({ installed: false, message: "" }),
  getAlgoPrefs: vi.fn().mockResolvedValue({ custom: false }),
  daemonSupportsUnboundGraceful: vi.fn().mockResolvedValue(false),
  daemonStopUnboundGraceful: vi.fn().mockResolvedValue("Request accepted; exit unverified"),
}));
vi.mock("../api", () => ({ api: mockApi, getDaemonUrl: () => "", reportError: vi.fn() }));
vi.mock("../i18n", () => ({ useI18n: () => ({ t: (text: string) => text }) }));
vi.mock("../theme", () => ({ useTheme: () => ({ theme: "dark", setTheme: vi.fn() }), THEMES: [] }));
vi.mock("./LanguageSwitcher", () => ({ default: () => null }));
vi.mock("./HighlightSettings", () => ({ default: () => null }));
vi.mock("./RedactionSettings", () => ({ default: () => null }));

beforeEach(() => {
  vi.clearAllMocks();
  mockApi.secretsStatus.mockResolvedValue({ initialized: false, unlocked: false });
  mockApi.daemonSupportsUnboundGraceful.mockResolvedValue(false);
});

function openSettings() {
  const noop = () => {};
  render(<><ConfirmHost /><SettingsMenu gateStatus={null} gateBusy={false} gateCheckedAt={null}
    daemonHealth={null} daemonHealthCheckedAt={null} onGateToggle={noop}
    onGateRefresh={noop} onDaemonHealthRefresh={noop} onImportConfig={noop} onOpenSetup={noop} /></>);
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
}

it("requires matching confirmation before initializing the credential store", async () => {
  openSettings();
  await waitFor(() => expect(mockApi.secretsStatus).toHaveBeenCalled());
  fireEvent.change(screen.getByPlaceholderText("Master password"), { target: { value: "synthetic-one" } });
  fireEvent.change(screen.getByPlaceholderText("Confirm master password"), { target: { value: "synthetic-other" } });
  fireEvent.click(screen.getByRole("button", { name: "Set master password" }));
  expect(screen.getByText("Master passwords do not match")).toBeTruthy();
  expect(mockApi.secretsUnlock).not.toHaveBeenCalled();
  fireEvent.change(screen.getByPlaceholderText("Confirm master password"), { target: { value: "synthetic-one" } });
  fireEvent.click(screen.getByRole("button", { name: "Set master password" }));
  await waitFor(() => expect(mockApi.secretsUnlock).toHaveBeenCalledWith("synthetic-one"));
  expect(await screen.findByText("Master password set")).toBeTruthy();
  expect(screen.queryByPlaceholderText("Confirm master password")).toBeNull();
});

it("requires explicit confirmation before requesting an unbound shutdown", async () => {
  mockApi.daemonSupportsUnboundGraceful.mockResolvedValue(true);
  openSettings();
  fireEvent.click(await screen.findByRole("button", { name: "Request unverified shutdown" }));
  expect(screen.getByText(/may stop the wrong daemon and expose the admin token/)).toBeTruthy();
  expect((screen.getByRole("button", { name: "Request unverified shutdown" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.pointerDown(screen.getByRole("button", { name: "Cancel" }));
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(mockApi.daemonStopUnboundGraceful).not.toHaveBeenCalled();
  await waitFor(() => expect((screen.getByRole("button", { name: "Request unverified shutdown" }) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: "Request unverified shutdown" }));
  fireEvent.pointerDown(screen.getByRole("button", { name: "Request shutdown" }));
  fireEvent.click(screen.getByRole("button", { name: "Request shutdown" }));
  await waitFor(() => expect(mockApi.daemonStopUnboundGraceful).toHaveBeenCalledTimes(1));
  expect(await screen.findByText("Request accepted; exit unverified")).toBeTruthy();
});

it("keeps the existing change-password flow for an initialized store", async () => {
  mockApi.secretsStatus.mockResolvedValue({ initialized: true, unlocked: true });
  openSettings();
  const input = await screen.findByPlaceholderText("New master password");
  expect(screen.queryByPlaceholderText("Confirm master password")).toBeNull();
  fireEvent.change(input, { target: { value: "synthetic-new" } });
  fireEvent.click(screen.getByRole("button", { name: "Change master password" }));
  await waitFor(() => expect(mockApi.secretsChangePassword).toHaveBeenCalledWith("synthetic-new"));
});
