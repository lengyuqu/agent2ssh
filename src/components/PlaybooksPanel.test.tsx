import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { HostProfile, Playbook } from "../types";
import PlaybooksPanel from "./PlaybooksPanel";
import { ToastProvider } from "./ui/toast";

const mockApi = vi.hoisted(() => ({
  setTrayLabels: vi.fn().mockResolvedValue(undefined),
  listPlaybooks: vi.fn(),
  savePlaybook: vi.fn(),
  deletePlaybook: vi.fn(),
  runPlaybook: vi.fn(),
}));

vi.mock("../api", () => ({
  api: mockApi,
  reportError: vi.fn(),
}));

const HOSTS: HostProfile[] = [
  { name: "web-01", host: "10.0.0.1", group: "default" },
  { name: "db-01", host: "10.0.0.2", group: "default" },
];

const PLAYBOOK: Playbook = {
  name: "health-check",
  description: "Check disk and uptime",
  tags: ["ops"],
  risk_override: null,
  steps: ["uptime"],
  advanced_steps: null,
};

function renderPanel(initialHost?: string) {
  return render(
    <I18nProvider>
      <ToastProvider>
        <PlaybooksPanel hosts={HOSTS} initialHost={initialHost} />
      </ToastProvider>
    </I18nProvider>,
  );
}

describe("PlaybooksPanel", () => {
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem("agent2ssh.language", "en");
    vi.clearAllMocks();
    mockApi.listPlaybooks.mockResolvedValue([PLAYBOOK]);
    mockApi.savePlaybook.mockImplementation(async (pb: Playbook) => pb);
  });

  it("pre-selects initialHost in the run bar when it matches an SSH host", async () => {
    renderPanel("db-01");
    fireEvent.click(await screen.findByRole("button", { name: "Run" }));
    const select = screen.getByRole("combobox") as HTMLSelectElement;
    expect(select.value).toBe("db-01");
  });

  it("opens the editor in a slide-over drawer and applies YAML edits when switching back to Steps", async () => {
    renderPanel();
    fireEvent.click(await screen.findByRole("button", { name: "Edit" }));
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Edit playbook: health-check" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "YAML" }));
    const yamlBox = screen.getByRole("textbox", { name: "YAML" }) as HTMLTextAreaElement;
    fireEvent.change(yamlBox, {
      target: {
        value: "name: health-check\ndescription: Updated\ntags:\n  - ops\nsteps:\n  - df -h\n",
      },
    });

    // Clicking the Steps tab syncs YAML instead of showing the stale visual form (L2-62).
    fireEvent.click(screen.getByRole("button", { name: "Steps" }));
    await waitFor(() => {
      expect(screen.getByDisplayValue("df -h")).toBeTruthy();
    });
    expect(screen.getByDisplayValue("Updated")).toBeTruthy();
  });

  it("stays on YAML with an error if invalid YAML is switched to Steps", async () => {
    renderPanel();
    fireEvent.click(await screen.findByRole("button", { name: "Edit" }));
    fireEvent.click(screen.getByRole("button", { name: "YAML" }));
    const yamlBox = screen.getByRole("textbox", { name: "YAML" });
    fireEvent.change(yamlBox, { target: { value: "not: [valid" } });

    fireEvent.click(screen.getByRole("button", { name: "Steps" }));
    expect(screen.getByRole("button", { name: "Apply YAML to step editor" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Add step" })).toBeNull();
  });
});
