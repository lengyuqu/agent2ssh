import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { HostProfile } from "../types";
import ExecPanel from "./ExecPanel";
import { ToastProvider } from "./ui/toast";

const { classifyRisk } = vi.hoisted(() => ({
  classifyRisk: vi.fn().mockResolvedValue("low"),
}));

vi.mock("../api", () => ({
  api: {
    setTrayLabels: vi.fn().mockResolvedValue(undefined),
    classifyRisk: (...args: unknown[]) => classifyRisk(...args),
  },
  reportError: vi.fn(),
}));

const HOSTS: HostProfile[] = [
  { name: "web", host: "web.internal", group: "default", user: "root", port: 22 },
];

function renderPanel(props: Partial<ComponentProps<typeof ExecPanel>> = {}) {
  const onExecComplete = vi.fn();
  const view = render(
    <I18nProvider>
      <ToastProvider>
        <ExecPanel hosts={HOSTS} onExecComplete={onExecComplete} initialHost="web" {...props} />
      </ToastProvider>
    </I18nProvider>,
  );
  return { onExecComplete, ...view };
}

function commandField(): HTMLTextAreaElement {
  return screen.getByLabelText("Command") as HTMLTextAreaElement;
}

describe("ExecPanel initial command", () => {
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem("agent2ssh.language", "en");
    classifyRisk.mockClear();
    classifyRisk.mockResolvedValue("low");
  });

  it("fills docker ps, focuses the end, toasts, and previews risk", async () => {
    renderPanel({ initialCommand: "docker ps" });
    const field = commandField();
    expect(field.value).toBe("docker ps");
    expect(document.activeElement).toBe(field);
    expect(field.selectionStart).toBe("docker ps".length);
    expect(field.selectionEnd).toBe("docker ps".length);
    expect(screen.getByText("Loaded pending command: docker ps")).toBeTruthy();

    await waitFor(() => {
      expect(classifyRisk).toHaveBeenCalledWith("docker ps", "web");
    });
    expect(screen.getByText("low")).toBeTruthy();
  });

  it("replaces an unexecuted draft without a confirm", () => {
    const { rerender } = renderPanel();
    const field = commandField();
    expect(field.value).toBe("uname -a");
    fireEvent.change(field, { target: { value: "echo draft" } });

    rerender(
      <I18nProvider>
        <ToastProvider>
          <ExecPanel
            hosts={HOSTS}
            onExecComplete={vi.fn()}
            initialHost="web"
            initialCommand="docker ps"
          />
        </ToastProvider>
      </I18nProvider>,
    );

    expect(commandField().value).toBe("docker ps");
    expect(screen.queryByText("Confirm execution")).toBeNull();
  });

  it("replaces the draft again when the same command is re-sent", () => {
    const { rerender } = renderPanel({ initialCommand: "docker ps", carryEpoch: 1 });
    fireEvent.change(commandField(), { target: { value: "echo draft" } });

    rerender(
      <I18nProvider>
        <ToastProvider>
          <ExecPanel
            hosts={HOSTS}
            onExecComplete={vi.fn()}
            initialHost="web"
            initialCommand="docker ps"
            carryEpoch={2}
          />
        </ToastProvider>
      </I18nProvider>,
    );

    expect(commandField().value).toBe("docker ps");
  });

  it("ignores an empty incoming command", () => {
    const { rerender } = renderPanel();
    fireEvent.change(commandField(), { target: { value: "echo draft" } });
    rerender(
      <I18nProvider>
        <ToastProvider>
          <ExecPanel
            hosts={HOSTS}
            onExecComplete={vi.fn()}
            initialHost="web"
            initialCommand="   "
          />
        </ToastProvider>
      </I18nProvider>,
    );
    expect(commandField().value).toBe("echo draft");
    expect(screen.queryByText(/Loaded pending command/)).toBeNull();
  });

  it("grows the textarea for a multiline command", () => {
    const command = "docker ps\n-a\n--format json";
    renderPanel({ initialCommand: command });
    const field = commandField();
    expect(field.value).toBe(command);
    expect(field.rows).toBe(3);
  });
});
