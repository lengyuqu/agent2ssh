import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { ExecMultiResult, HostProfile } from "../types";
import MultiExecPanel from "./MultiExecPanel";
import { ToastProvider } from "./ui/toast";

const { classifyRisk, execMulti } = vi.hoisted(() => ({
  classifyRisk: vi.fn().mockResolvedValue("low"),
  execMulti: vi.fn(),
}));

vi.mock("../api", () => ({
  api: {
    setTrayLabels: vi.fn().mockResolvedValue(undefined),
    classifyRisk: (...args: unknown[]) => classifyRisk(...args),
    execMulti: (...args: unknown[]) => execMulti(...args),
  },
  reportError: vi.fn(),
}));

function host(name: string, group: string, tags: string[]): HostProfile {
  return { name, host: `${name}.internal`, group, user: "root", port: 22, tags };
}

const HOSTS: HostProfile[] = [
  host("web", "edge", ["prod"]),
  host("db", "data", ["prod", "db"]),
  host("dev", "edge", ["dev"]),
];

function row(name: string, exitCode: number, stdout: string, durationMs: number): ExecMultiResult {
  return {
    host: name,
    result: {
      host: name,
      command: "echo hi",
      exit_code: exitCode,
      stdout,
      stderr: "",
      duration_ms: durationMs,
      risk_level: "low",
    },
    error: null,
  };
}

const RESULTS: ExecMultiResult[] = [row("web", 0, "ok-web", 12), row("db", 1, "bad-db", 30)];

function renderPanel(props: Partial<ComponentProps<typeof MultiExecPanel>> = {}) {
  const onExecComplete = vi.fn();
  const view = render(
    <I18nProvider>
      <ToastProvider>
        <MultiExecPanel hosts={HOSTS} onExecComplete={onExecComplete} {...props} />
      </ToastProvider>
    </I18nProvider>,
  );
  return { onExecComplete, ...view };
}

function outputText(): string {
  const pres = document.querySelectorAll("pre");
  expect(pres).toHaveLength(1);
  return pres[0]?.textContent ?? "";
}

describe("MultiExecPanel", () => {
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem("agent2ssh.language", "en");
    classifyRisk.mockReset();
    classifyRisk.mockResolvedValue("low");
    execMulti.mockReset();
    execMulti.mockResolvedValue(RESULTS);
  });

  it("runs from tags alone when nothing is selected by hand", async () => {
    renderPanel();
    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "echo hi" } });
    fireEvent.change(screen.getByLabelText("Tags (comma-separated, optional)"), {
      target: { value: "db" },
    });
    expect(screen.getByText("1 host will run")).toBeTruthy();
    const run = screen.getByRole("button", { name: "Run on 1 host" });
    expect(run.hasAttribute("disabled")).toBe(false);
    fireEvent.click(run);
    await waitFor(() => {
      expect(execMulti).toHaveBeenCalledWith([], "echo hi", false, undefined, ["db"]);
    });
  });

  it("selects hosts from a group chip and a tag chip", () => {
    renderPanel();
    const hosts = screen.getByRole("group", { name: "Hosts" });
    fireEvent.click(within(screen.getByRole("group", { name: "Groups" })).getByRole("button", { name: "edge" }));
    expect(screen.getByText("2 hosts will run")).toBeTruthy();
    expect(within(hosts).getByRole("button", { name: "web" }).getAttribute("aria-pressed")).toBe("true");
    expect(within(hosts).getByRole("button", { name: "dev" }).getAttribute("aria-pressed")).toBe("true");
    expect(within(hosts).getByRole("button", { name: "db" }).getAttribute("aria-pressed")).toBe("false");

    fireEvent.click(within(screen.getByRole("group", { name: "Groups" })).getByRole("button", { name: "edge" }));
    expect(screen.getByText("0 hosts will run")).toBeTruthy();

    fireEvent.click(within(screen.getByRole("group", { name: "Tags" })).getByRole("button", { name: "prod" }));
    expect(screen.getByText("2 hosts will run")).toBeTruthy();
    expect(within(hosts).getByRole("button", { name: "web" }).getAttribute("aria-pressed")).toBe("true");
    expect(within(hosts).getByRole("button", { name: "db" }).getAttribute("aria-pressed")).toBe("true");
    expect(within(hosts).getByRole("button", { name: "dev" }).getAttribute("aria-pressed")).toBe("false");
  });

  it("shows one output pane, prefers the first failure, and can filter to failures", async () => {
    renderPanel();
    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "echo hi" } });
    fireEvent.click(screen.getByRole("button", { name: "Select all" }));
    fireEvent.click(screen.getByRole("button", { name: "Run on 3 hosts" }));
    await waitFor(() => {
      expect(screen.getByText("bad-db")).toBeTruthy();
    });

    expect(outputText()).toContain("bad-db");
    expect(outputText()).not.toContain("ok-web");

    const list = screen.getByRole("listbox", { name: "Results" });
    fireEvent.click(within(list).getByRole("option", { name: /web/ }));
    expect(outputText()).toContain("ok-web");
    expect(outputText()).not.toContain("bad-db");

    fireEvent.click(screen.getByRole("tab", { name: "Failed only (1)" }));
    const failed = screen.getByRole("listbox", { name: "Results" });
    expect(within(failed).queryByRole("option", { name: /web/ })).toBeNull();
    expect(within(failed).getByRole("option", { name: /db/ })).toBeTruthy();
    expect(outputText()).toContain("bad-db");

    fireEvent.click(screen.getByRole("tab", { name: "Output diff" }));
    expect(screen.queryByRole("listbox", { name: "Results" })).toBeNull();
    expect(screen.getByText("Compare hosts")).toBeTruthy();
  });

  it("asks before running a high-risk command", async () => {
    classifyRisk.mockResolvedValue("high");
    renderPanel();
    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "echo hi" } });
    fireEvent.click(within(screen.getByRole("group", { name: "Hosts" })).getByRole("button", { name: "web" }));
    fireEvent.click(screen.getByRole("button", { name: "Run on 1 host" }));
    expect(await screen.findByText("Confirm execution")).toBeTruthy();
    expect(execMulti).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Confirm and execute" }));
    await waitFor(() => {
      expect(execMulti).toHaveBeenCalledWith(["web"], "echo hi", true, undefined, undefined);
    });
  });

  it("renders stderr below stdout when both streams have output", async () => {
    execMulti.mockResolvedValue([
      {
        host: "web",
        result: {
          host: "web",
          command: "echo hi",
          exit_code: 1,
          stdout: "partial output",
          stderr: "permission denied",
          duration_ms: 15,
          risk_level: "low",
        },
        error: null,
      },
    ]);
    renderPanel();
    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "echo hi" } });
    fireEvent.click(within(screen.getByRole("group", { name: "Hosts" })).getByRole("button", { name: "web" }));
    fireEvent.click(screen.getByRole("button", { name: "Run on 1 host" }));

    expect(await screen.findByText("partial output")).toBeTruthy();
    expect(screen.getByText("permission denied")).toBeTruthy();
  });
});
