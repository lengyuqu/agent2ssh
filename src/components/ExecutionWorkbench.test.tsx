import { fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps, ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import ExecutionWorkbench, { EXEC_MODE_STORAGE_KEY } from "./ExecutionWorkbench";

vi.mock("../api", () => ({
  api: { setTrayLabels: vi.fn().mockResolvedValue(undefined) },
  reportError: vi.fn(),
}));

vi.mock("./ExecPanel", () => ({
  default: (props: { initialCommand?: string; initialHost?: string; carryEpoch?: number }) => (
    <div data-testid="exec-panel">
      {props.initialHost ?? ""}|{props.initialCommand ?? ""}|{props.carryEpoch ?? 0}
    </div>
  ),
}));

vi.mock("./MultiExecPanel", () => ({
  default: () => <div data-testid="multi-exec-panel" />,
}));

vi.mock("./PlaybooksPanel", () => ({
  default: () => <div data-testid="playbooks-panel" />,
}));

function tree(props: Partial<ComponentProps<typeof ExecutionWorkbench>> = {}): ReactNode {
  return (
    <I18nProvider>
      <ExecutionWorkbench hosts={[]} onExecComplete={vi.fn()} {...props} />
    </I18nProvider>
  );
}

describe("ExecutionWorkbench", () => {
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem("agent2ssh.language", "en");
  });

  it("mounts only the active panel and switches tabs", () => {
    render(tree());
    expect(screen.getByTestId("exec-panel")).toBeTruthy();
    expect(screen.queryByTestId("multi-exec-panel")).toBeNull();
    expect(screen.queryByTestId("playbooks-panel")).toBeNull();
    expect(screen.getByRole("tab", { name: "Single Host" }).getAttribute("aria-selected")).toBe(
      "true",
    );

    fireEvent.click(screen.getByRole("tab", { name: "Multi-Host" }));
    expect(screen.getByTestId("multi-exec-panel")).toBeTruthy();
    expect(screen.queryByTestId("exec-panel")).toBeNull();
    expect(screen.queryByTestId("playbooks-panel")).toBeNull();

    fireEvent.click(screen.getByRole("tab", { name: "Playbooks" }));
    expect(screen.getByTestId("playbooks-panel")).toBeTruthy();
    expect(screen.queryByTestId("exec-panel")).toBeNull();
    expect(screen.queryByTestId("multi-exec-panel")).toBeNull();
    expect(localStorage.getItem(EXEC_MODE_STORAGE_KEY)).toBe("playbooks");
  });

  it("restores the last mode from localStorage", () => {
    localStorage.setItem(EXEC_MODE_STORAGE_KEY, "multi");
    render(tree());
    expect(screen.getByTestId("multi-exec-panel")).toBeTruthy();
    expect(screen.queryByTestId("exec-panel")).toBeNull();
  });

  it("opens on single host when an outside jump asks for it", () => {
    localStorage.setItem(EXEC_MODE_STORAGE_KEY, "multi");
    const onPreferredModeApplied = vi.fn();
    const { rerender } = render(tree());
    expect(screen.getByTestId("multi-exec-panel")).toBeTruthy();

    rerender(
      tree({
        preferredMode: "single",
        initialHost: "web",
        initialCommand: "docker ps",
        carryEpoch: 1,
        onPreferredModeApplied,
      }),
    );

    expect(screen.getByTestId("exec-panel").textContent).toBe("web|docker ps|1");
    expect(screen.queryByTestId("multi-exec-panel")).toBeNull();
    expect(onPreferredModeApplied).toHaveBeenCalledTimes(1);
  });

  it("switches to single host when a new command arrives", () => {
    localStorage.setItem(EXEC_MODE_STORAGE_KEY, "multi");
    const { rerender } = render(tree());
    rerender(tree({ initialCommand: "docker ps" }));
    expect(screen.getByTestId("exec-panel").textContent).toContain("docker ps");
    expect(screen.queryByTestId("multi-exec-panel")).toBeNull();
  });

  it("reports a tab change through onModeChange", () => {
    const onModeChange = vi.fn();
    render(tree({ onModeChange }));
    fireEvent.click(screen.getByRole("tab", { name: "Multi-Host" }));
    expect(onModeChange).toHaveBeenCalledWith("multi");
    fireEvent.click(screen.getByRole("tab", { name: "Playbooks" }));
    expect(onModeChange).toHaveBeenCalledWith("playbooks");
    fireEvent.click(screen.getByRole("tab", { name: "Single Host" }));
    expect(onModeChange).toHaveBeenCalledWith("single");
  });

  it("keeps the playbooks route on playbooks when a command is already carried", () => {
    localStorage.setItem(EXEC_MODE_STORAGE_KEY, "single");
    render(tree({ preferredMode: "playbooks", initialCommand: "docker ps", carryEpoch: 3 }));
    expect(screen.getByTestId("playbooks-panel")).toBeTruthy();
    expect(screen.queryByTestId("exec-panel")).toBeNull();
  });
});
