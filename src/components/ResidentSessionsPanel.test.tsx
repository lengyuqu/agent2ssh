import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { ResidentSessionSummary } from "../types";
import ResidentSessionsPanel from "./ResidentSessionsPanel";
import { ToastProvider } from "./ui/toast";

const mockApi = vi.hoisted(() => ({
  setTrayLabels: vi.fn().mockResolvedValue(undefined),
  residentListDaemon: vi.fn(),
  residentCloseDaemon: vi.fn(),
}));

vi.mock("../api", () => ({
  api: mockApi,
  reportError: vi.fn(),
}));

function session(idleSecs: number): ResidentSessionSummary {
  return {
    id: "sess-1",
    host: "local:zsh",
    label: "default",
    idle_secs: idleSecs,
    dead: false,
  };
}

function renderPanel() {
  return render(
    <I18nProvider>
      <ToastProvider>
        <ResidentSessionsPanel />
      </ToastProvider>
    </I18nProvider>,
  );
}

describe("ResidentSessionsPanel polling", () => {
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem("agent2ssh.language", "en");
    vi.clearAllMocks();
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    mockApi.residentListDaemon.mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => "visible",
    });
  });

  it("polls every 5s only while sessions exist and pauses when hidden", async () => {
    mockApi.residentListDaemon.mockResolvedValue([]);
    const empty = renderPanel();
    await waitFor(() => expect(mockApi.residentListDaemon).toHaveBeenCalledTimes(1));

    await act(async () => {
      await vi.advanceTimersByTimeAsync(15_000);
    });
    expect(mockApi.residentListDaemon).toHaveBeenCalledTimes(1);
    empty.unmount();

    mockApi.residentListDaemon.mockResolvedValueOnce([session(5)]).mockResolvedValueOnce([session(10)]);
    renderPanel();
    expect(await screen.findByText(/5s idle/)).toBeTruthy();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(await screen.findByText(/10s idle/)).toBeTruthy();

    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => "hidden",
    });
    document.dispatchEvent(new Event("visibilitychange"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(15_000);
    });
    expect(mockApi.residentListDaemon).toHaveBeenCalledTimes(3);
  });
});
