import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../api";
import { I18nProvider } from "../i18n";
import LocalEnvironmentsCard from "./LocalEnvironmentsCard";

vi.mock("../api", () => ({
  api: {
    setTrayLabels: vi.fn().mockResolvedValue(undefined),
    listLocalEnvironments: vi.fn(),
  },
}));

const environments = [
  {
    id: "local",
    builtin: true,
    base: null,
    distro: null,
    cwd: null,
    env_count: 0,
    description: "default shell",
    platform: "macOS",
  },
  {
    id: "pwsh",
    builtin: true,
    base: null,
    distro: null,
    cwd: null,
    env_count: 0,
    description: "PowerShell",
    platform: "macOS",
  },
  {
    id: "local:zsh",
    builtin: true,
    base: null,
    distro: null,
    cwd: null,
    env_count: 0,
    description: "Zsh",
    platform: "macOS",
  },
];

function renderCard(onExecuteLocal = vi.fn()) {
  const view = render(
    <I18nProvider>
      <LocalEnvironmentsCard onExecuteLocal={onExecuteLocal} />
    </I18nProvider>
  );
  return { ...view, onExecuteLocal };
}

function runButton(id: string) {
  const item = screen.getByText(id).closest(".rounded-lg");
  if (!item) throw new Error(`missing environment ${id}`);
  return within(item as HTMLElement).getByRole("button", { name: "Go to execute" });
}

describe("LocalEnvironmentsCard", () => {
  beforeEach(() => {
    localStorage.removeItem("agent2ssh.localEnvExpanded");
    vi.mocked(api.listLocalEnvironments).mockReset();
    vi.mocked(api.listLocalEnvironments).mockResolvedValue(environments as never);
  });

  it("starts collapsed and remembers the expanded state", async () => {
    const first = renderCard();

    expect(await screen.findByText("macOS")).toBeTruthy();
    expect(screen.getByText("3 discovered")).toBeTruthy();
    expect(screen.getByText("local:*")).toBeTruthy();
    expect(screen.queryByText("PowerShell")).toBeNull();
    expect(screen.queryByRole("button", { name: "Go to execute" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /Local environments/ }));
    expect(screen.getByText("PowerShell")).toBeTruthy();
    expect(localStorage.getItem("agent2ssh.localEnvExpanded")).toBe("1");

    fireEvent.click(screen.getByRole("button", { name: /Local environments/ }));
    expect(screen.queryByText("PowerShell")).toBeNull();
    expect(localStorage.getItem("agent2ssh.localEnvExpanded")).toBe("0");

    first.unmount();
    localStorage.setItem("agent2ssh.localEnvExpanded", "1");
    renderCard();
    expect(await screen.findByText("Zsh")).toBeTruthy();
  });

  it("runs the plain local target as local and prefixes other ids", async () => {
    localStorage.setItem("agent2ssh.localEnvExpanded", "1");
    const { onExecuteLocal } = renderCard();

    expect(await screen.findByText("PowerShell")).toBeTruthy();
    fireEvent.click(runButton("local"));
    fireEvent.click(runButton("pwsh"));
    fireEvent.click(runButton("local:zsh"));

    expect(onExecuteLocal).toHaveBeenNthCalledWith(1, "local");
    expect(onExecuteLocal).toHaveBeenNthCalledWith(2, "local:pwsh");
    expect(onExecuteLocal).toHaveBeenNthCalledWith(3, "local:zsh");
  });
});
