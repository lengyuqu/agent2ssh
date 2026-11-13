import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import SettingsMenu from "./SettingsMenu";
import { ConfirmHost } from "./ui/dialog";
import type { DaemonDiagnosis } from "../types";

const mockApi = vi.hoisted(() => ({
  getTelemetryEnabled: vi.fn().mockResolvedValue(false),
  secretsStatus: vi.fn(),
  secretsUnlock: vi.fn().mockResolvedValue(undefined),
  secretsChangePassword: vi.fn().mockResolvedValue(undefined),
  getCliPathStatus: vi.fn().mockResolvedValue({ installed: false, message: "" }),
  getAlgoPrefs: vi.fn().mockResolvedValue({ custom: false }),
  daemonSupportsUnboundGraceful: vi.fn().mockResolvedValue(false),
  daemonStopUnboundGraceful: vi.fn().mockResolvedValue("Request accepted; exit unverified"),
  daemonRestart: vi.fn(),
  daemonStatus: vi.fn().mockResolvedValue({ running: false, pid: null, message: "", diagnosis: null }),
}));
vi.mock("../api", () => ({ api: mockApi, getDaemonUrl: () => "", reportError: vi.fn() }));
vi.mock("../i18n", () => ({
  // Interpolate like the real t(): the diagnosis line passes a {verdict} var.
  useI18n: () => ({
    t: (text: string, vars?: Record<string, string | number>) =>
      vars
        ? text.replace(/\{([A-Za-z0-9_]+)\}/g, (_match, name: string) =>
            vars[name] === undefined ? `{${name}}` : String(vars[name])
          )
        : text,
  }),
}));
vi.mock("../theme", () => ({ useTheme: () => ({ theme: "dark", setTheme: vi.fn() }), THEMES: [] }));
vi.mock("./LanguageSwitcher", () => ({ default: () => null }));
vi.mock("./HighlightSettings", () => ({ default: () => null }));
vi.mock("./RedactionSettings", () => ({ default: () => null }));

beforeEach(() => {
  vi.clearAllMocks();
  mockApi.secretsStatus.mockResolvedValue({ initialized: false, unlocked: false });
  mockApi.daemonSupportsUnboundGraceful.mockResolvedValue(false);
  mockApi.daemonRestart.mockResolvedValue({ message: "Restarted" });
  // clearAllMocks keeps implementations, so re-pin the default in case an
  // earlier test overrode it with a rejection.
  mockApi.daemonStatus.mockResolvedValue({ running: false, pid: null, message: "", diagnosis: null });
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
  await waitFor(() =>   expect(mockApi.daemonStopUnboundGraceful).toHaveBeenCalledTimes(1));
  expect(await screen.findByText("Request accepted; exit unverified")).toBeTruthy();
});

// A full diagnosis so a test can name exactly what the panel should read out
// of `daemon_status`: the verdict, the recorded PID, the OS observation and
// the lock state — the real record, not the /health probe.
function diagnosisFor(overrides: Partial<DaemonDiagnosis> = {}): DaemonDiagnosis {
  return {
    config_dir: "/cfg",
    pid_path: "/cfg/daemon.pid",
    token_path: "/cfg/daemon.token",
    log_path: "/cfg/daemon.log",
    pid_lock: "free",
    record: { state: "pid", pid: 4242 },
    observation: { state: "live", start_time: 1700000000 },
    health: { state: "reporting", pid: 4242, same_pid: true, config_dir_matches: true },
    token: "not-checked",
    verdict: "unverified",
    next_step: "Inspect the process out of band.",
    ...overrides,
  };
}

it("shows the daemon_status record, observation and lock instead of only the health probe", async () => {
  mockApi.daemonStatus.mockResolvedValue({
    running: true,
    pid: 4242,
    message: "",
    diagnosis: diagnosisFor({ pid_lock: "held" }),
  });
  openSettings();
  expect(await screen.findByText("Diagnosis: identity unverified")).toBeTruthy();
  expect(screen.getByText("Record PID: 4242")).toBeTruthy();
  expect(screen.getByText("The OS observes the recorded PID live")).toBeTruthy();
  expect(screen.getByText("Another start or stop is holding the PID lock")).toBeTruthy();
});

// #24: a cross-account daemon denies inspection. The panel must relay that as
// "identity unknown", never as "the daemon exited".
it("relays a denied observation as unknown identity, not as an exit", async () => {
  mockApi.daemonStatus.mockResolvedValue({
    running: true,
    pid: 4242,
    message: "",
    diagnosis: diagnosisFor({
      observation: { state: "denied", reason: "OpenProcess denied (error 5)" },
    }),
  });
  openSettings();
  const line = await screen.findByText(/denied inspection of this process/);
  expect(line.textContent).toContain("not evidence it exited");
});

it("names the observation states that contradict or erode the record", async () => {
  mockApi.daemonStatus.mockResolvedValue({
    running: false,
    pid: null,
    message: "",
    diagnosis: diagnosisFor({
      observation: { state: "absent" },
      health: { state: "inactive" },
      verdict: "stale",
    }),
  });
  openSettings();
  expect(await screen.findByText("The OS reports the recorded PID absent")).toBeTruthy();
});

// #24: when `daemon_status` itself fails or has nothing to say, the panel
// states that instead of leaving only the legacy /health line.
it("says the read-only diagnosis is unavailable instead of staying silent", async () => {
  mockApi.daemonStatus.mockResolvedValue({ running: false, pid: null, message: "", diagnosis: null });
  openSettings();
  expect(await screen.findByText("Read-only diagnosis unavailable")).toBeTruthy();
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

// #21: the panel consumes `daemon_status` (L2-142 was "zero call sites"). The
// verdict line must appear once per menu open, carry the backend's next step
// when the verdict is not healthy, and re-fetch on the health refresh button.
function diagnosisResult(verdict: string, next_step: string, pid: number) {
  return {
    running: verdict === "healthy",
    pid,
    message: `${verdict}: ${next_step}`,
    diagnosis: {
      config_dir: "C:\\synthetic\\agent2ssh",
      pid_path: "C:\\synthetic\\agent2ssh\\daemon.pid",
      token_path: "C:\\synthetic\\agent2ssh\\daemon.token",
      log_path: "C:\\synthetic\\agent2ssh\\daemon.log",
      pid_lock: "free",
      record: { state: "pid", pid },
      observation: { state: "absent" },
      health: { state: "inactive" },
      token: "not-checked",
      verdict,
      next_step,
    },
  };
}

it("shows the daemon_status verdict and its next step for a stale record", async () => {
  const nextStep =
    "Run 'agent2ssh daemon clear-stale --expect-pid 4242' to remove the record only if it still matches and the PID stays absent. No process is signalled.";
  mockApi.daemonStatus.mockResolvedValue(diagnosisResult("stale", nextStep, 4242));
  openSettings();
  expect(await screen.findByText("Diagnosis: stale PID record")).toBeTruthy();
  expect(screen.getByText(nextStep)).toBeTruthy();
  expect(mockApi.daemonStatus).toHaveBeenCalledTimes(1);
  // The health refresh button re-fetches the diagnosis alongside /health.
  fireEvent.click(await screen.findByRole("button", { name: "Refresh daemon health" }));
  await waitFor(() => expect(mockApi.daemonStatus).toHaveBeenCalledTimes(2));
});

it("shows the verdict without the next step when the daemon is healthy", async () => {
  mockApi.daemonStatus.mockResolvedValue(
    diagnosisResult(
      "healthy",
      "Nothing to do. Health and the admin token agree with the record, but process identity is still unbound (see #22).",
      1
    )
  );
  openSettings();
  expect(await screen.findByText("Diagnosis: health checks passed")).toBeTruthy();
  expect(screen.getByText("Health checks do not verify daemon process identity.")).toBeTruthy();
  expect(screen.queryByText(/Nothing to do/)).toBeNull();
});

it("renders no diagnosis line when daemon_status itself fails", async () => {
  mockApi.daemonStatus.mockRejectedValue(new Error("ipc broken"));
  openSettings();
  await waitFor(() => expect(mockApi.daemonStatus).toHaveBeenCalled());
  expect(screen.queryByText(/^Diagnosis:/)).toBeNull();
});

it("ignores an older diagnosis that finishes after a newer refresh", async () => {
  let finishFirst!: (value: ReturnType<typeof diagnosisResult>) => void;
  mockApi.daemonStatus
    .mockImplementationOnce(() => new Promise((resolve) => { finishFirst = resolve; }))
    .mockResolvedValueOnce(diagnosisResult("stale", "Inspect the stale record.", 42));
  openSettings();
  await waitFor(() => expect(mockApi.daemonStatus).toHaveBeenCalledTimes(1));
  fireEvent.click(screen.getByRole("button", { name: "Refresh daemon health" }));
  expect(await screen.findByText("Diagnosis: stale PID record")).toBeTruthy();
  await act(async () => finishFirst(diagnosisResult("healthy", "Old result", 1)));
  expect(screen.getByText("Diagnosis: stale PID record")).toBeTruthy();
  expect(screen.queryByText("Diagnosis: health checks passed")).toBeNull();
});

it("ignores a diagnosis from before the menu was closed and reopened", async () => {
  let finishFirst!: (value: ReturnType<typeof diagnosisResult>) => void;
  mockApi.daemonStatus
    .mockImplementationOnce(() => new Promise((resolve) => { finishFirst = resolve; }))
    .mockResolvedValueOnce(diagnosisResult("stale", "Inspect the stale record.", 42));
  openSettings();
  await waitFor(() => expect(mockApi.daemonStatus).toHaveBeenCalledTimes(1));
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(await screen.findByText("Diagnosis: stale PID record")).toBeTruthy();
  await act(async () => finishFirst(diagnosisResult("healthy", "Old result", 1)));
  expect(screen.getByText("Diagnosis: stale PID record")).toBeTruthy();
});

it("refreshes the diagnosis after a failed daemon action", async () => {
  mockApi.daemonStatus
    .mockResolvedValueOnce(diagnosisResult("healthy", "Old result", 1))
    .mockResolvedValueOnce(diagnosisResult("stale", "Inspect the stale record.", 42));
  mockApi.daemonRestart.mockRejectedValue(new Error("restart failed"));
  openSettings();
  await screen.findByText("Diagnosis: health checks passed");
  fireEvent.click(screen.getByRole("button", { name: "Restart daemon" }));
  expect(await screen.findByText("Error: restart failed")).toBeTruthy();
  expect(await screen.findByText("Diagnosis: stale PID record")).toBeTruthy();
});

it("does not show a pre-action diagnosis while a restart is in progress", async () => {
  let finishRestart!: (value: { message: string }) => void;
  mockApi.daemonStatus.mockResolvedValue(diagnosisResult("stale", "Inspect the record.", 42));
  mockApi.daemonRestart.mockImplementationOnce(() => new Promise((resolve) => { finishRestart = resolve; }));
  openSettings();
  await screen.findByText("Diagnosis: stale PID record");
  fireEvent.click(screen.getByRole("button", { name: "Restart daemon" }));
  expect(screen.queryByText(/^Diagnosis:/)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Refresh daemon health" }));
  await waitFor(() => expect(mockApi.daemonStatus).toHaveBeenCalledTimes(1));
  expect(screen.queryByText(/^Diagnosis:/)).toBeNull();
  await act(async () => finishRestart({ message: "Restarted" }));
  expect(await screen.findByText("Diagnosis: stale PID record", {}, { timeout: 2000 })).toBeTruthy();
});
