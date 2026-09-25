import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { ContainerDiscoveryTarget, ContainerRuntimeInfo } from "../types";
import ContainersPanel from "./ContainersPanel";
import { ToastProvider } from "./ui/toast";

// The panel reads both bridges on mount (discovery + runtime states) and can
// launch a terminal. Stub the whole api module so no test reaches Tauri.
const mockApi = vi.hoisted(() => ({
  setTrayLabels: vi.fn().mockResolvedValue(undefined),
  discoverContainers: vi.fn(),
  containerRuntimes: vi.fn(),
  launchContainerTarget: vi.fn(),
}));

vi.mock("../api", () => ({
  api: mockApi,
  reportError: vi.fn(),
}));

/** A stopped wslc container, as `agent2ssh containers list --all` reports it. */
const wslcTarget: ContainerDiscoveryTarget = {
  id: "wslc_exec:local:488611800bc0",
  platform: "wslc",
  context: "local",
  container_id: "488611800bc0",
  container_name: "a2s-probe",
  image: "alpine:latest",
  status: "Created",
  exec_args: ["exec", "-it", "488611800bc0", "/bin/sh"],
  // Contains a space, which is why the displayed command must quote it.
  exec_binary: "C:\\Program Files\\WSL\\wslc.exe",
  namespace: null,
};

const dockerTarget: ContainerDiscoveryTarget = {
  id: "docker_exec:default:abc123def456",
  platform: "docker",
  context: "default",
  container_id: "abc123def456",
  container_name: "web-server",
  image: "nginx:latest",
  status: "Up 2 hours",
  exec_args: ["exec", "-it", "abc123def456", "/bin/sh"],
  exec_binary: "/usr/local/bin/docker",
  namespace: null,
};

const runtimes: ContainerRuntimeInfo[] = [
  {
    runtime: "docker",
    display_name: "Docker",
    binary: null,
    supported: true,
    available: false,
    selected: false,
    unsupported_reason: null,
  },
  {
    runtime: "wslc",
    display_name: "WSL containers (wslc)",
    binary: "C:\\Program Files\\WSL\\wslc.exe",
    supported: true,
    available: true,
    selected: true,
    unsupported_reason: null,
  },
  {
    runtime: "apple",
    display_name: "Apple container",
    binary: null,
    supported: false,
    available: false,
    selected: false,
    unsupported_reason: "Apple container requires macOS 26+ on Apple silicon",
  },
];

function renderPanel() {
  return render(
    <I18nProvider>
      <ToastProvider>
        <ContainersPanel />
      </ToastProvider>
    </I18nProvider>
  );
}

describe("ContainersPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockApi.setTrayLabels.mockResolvedValue(undefined);
    mockApi.discoverContainers.mockResolvedValue([]);
    mockApi.containerRuntimes.mockResolvedValue(runtimes);
    mockApi.launchContainerTarget.mockResolvedValue(undefined);
  });

  it("lists a target per discovered container with its runtime badge", async () => {
    mockApi.discoverContainers.mockResolvedValue([wslcTarget, dockerTarget]);
    renderPanel();

    await waitFor(() => expect(mockApi.discoverContainers).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("a2s-probe")).toBeTruthy();
    expect(screen.getByText("web-server")).toBeTruthy();
    // The platform badge says "wslc"; the runtime strip's own badge reads
    // "WSL containers (wslc)", so only the target's badge matches exactly.
    expect(screen.getByText("wslc")).toBeTruthy();
    // Both the platform badge and the runtime strip say "Docker" for this
    // target, so the row is identified by its image and status instead.
    expect(screen.getByText("nginx:latest · Up 2 hours")).toBeTruthy();
    // Running containers only, unless the user asks for the stopped ones.
    expect(mockApi.discoverContainers).toHaveBeenCalledWith({ all: false });
  });

  it("quotes a binary path that contains a space in the copyable command", async () => {
    mockApi.discoverContainers.mockResolvedValue([wslcTarget]);
    renderPanel();

    const command = await screen.findByText(
      '"C:\\Program Files\\WSL\\wslc.exe" exec -it 488611800bc0 /bin/sh'
    );
    expect(command).toBeTruthy();
  });

  it("copies the exec command to the clipboard", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    mockApi.discoverContainers.mockResolvedValue([dockerTarget]);
    renderPanel();

    const copy = await screen.findByLabelText("Copy exec command");
    fireEvent.click(copy);

    expect(writeText).toHaveBeenCalledWith(
      "/usr/local/bin/docker exec -it abc123def456 /bin/sh"
    );
  });

  it("launches the discovered argv in a terminal", async () => {
    mockApi.discoverContainers.mockResolvedValue([dockerTarget]);
    renderPanel();

    const launch = await screen.findByText("Open in terminal");
    fireEvent.click(launch);

    await waitFor(() =>
      expect(mockApi.launchContainerTarget).toHaveBeenCalledWith(
        "/usr/local/bin/docker",
        ["exec", "-it", "abc123def456", "/bin/sh"]
      )
    );
  });

  it("re-queries with all=true when stopped containers are included", async () => {
    renderPanel();
    await waitFor(() => expect(mockApi.discoverContainers).toHaveBeenCalledWith({ all: false }));

    fireEvent.click(screen.getByText("Include stopped"));

    await waitFor(() => expect(mockApi.discoverContainers).toHaveBeenCalledWith({ all: true }));
  });

  it("explains which runtimes are missing when the host has none", async () => {
    mockApi.containerRuntimes.mockResolvedValue(
      runtimes.map((info) => ({ ...info, available: false, binary: null, selected: false }))
    );
    renderPanel();

    expect(
      await screen.findByText("No container runtime is available on this host")
    ).toBeTruthy();
    // The install guidance names all three runtimes, not just Docker.
    const guidance = screen.getByText(
      "Install Docker, WSL containers (wslc) or Apple container on this host, then refresh. Only the runtimes this platform supports are listed."
    );
    expect(guidance).toBeTruthy();
    // An unsupported runtime still says why, instead of looking like a missing install.
    expect(screen.getByText("Apple container")).toBeTruthy();
    expect(screen.getByText("unsupported here")).toBeTruthy();
  });

  it("distinguishes 'no containers running' from 'no runtime installed'", async () => {
    renderPanel();

    expect(await screen.findByText("No containers found")).toBeTruthy();
    expect(
      screen.getByText("Start a container, then refresh — or include stopped ones.")
    ).toBeTruthy();
    expect(screen.getByText("detected (auto)")).toBeTruthy();
    expect(screen.getByText("not installed")).toBeTruthy();
  });

  it("reports a failed launch instead of pretending it opened", async () => {
    mockApi.discoverContainers.mockResolvedValue([dockerTarget]);
    mockApi.launchContainerTarget.mockRejectedValue("docker is not a runtime on this host");
    renderPanel();

    fireEvent.click(await screen.findByText("Open in terminal"));

    await waitFor(() => expect(mockApi.launchContainerTarget).toHaveBeenCalled());
    // The launch button must leave its busy label behind and stay clickable.
    expect(await screen.findByText("Open in terminal")).toBeTruthy();
  });
});
