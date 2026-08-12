import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { ToastProvider } from "./ui/toast";
import SnippetsDialog from "./SnippetsDialog";

const mockApi = vi.hoisted(() => ({
  setTrayLabels: vi.fn().mockResolvedValue(undefined),
  listSnippets: vi.fn(),
  saveSnippet: vi.fn(),
  deleteSnippet: vi.fn(),
}));

vi.mock("../api", () => ({
  api: mockApi,
  reportError: vi.fn(),
}));

const snippets = [
  { name: "disk", command: "df -h", description: "Show disk usage" },
  { name: "logs", command: "journalctl -f", description: "Follow service logs" },
];

function renderDialog(overrides: { canInsert?: boolean; onInsert?: (command: string) => void } = {}) {
  const onInsert = overrides.onInsert ?? vi.fn();
  const view = render(
    <I18nProvider>
      <ToastProvider>
        <SnippetsDialog
          open
          canInsert={overrides.canInsert ?? true}
          onClose={vi.fn()}
          onInsert={onInsert}
        />
      </ToastProvider>
    </I18nProvider>
  );
  return { ...view, onInsert };
}

describe("SnippetsDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockApi.setTrayLabels.mockResolvedValue(undefined);
    mockApi.listSnippets.mockResolvedValue(snippets);
    mockApi.saveSnippet.mockImplementation(async (snippet) => [...snippets, snippet]);
    mockApi.deleteSnippet.mockResolvedValue(true);
  });

  it("loads and searches snippets by description and command", async () => {
    renderDialog();
    await screen.findByText("disk");

    fireEvent.change(screen.getByLabelText("Search snippets"), {
      target: { value: "journalctl" },
    });

    await waitFor(() => {
      expect(screen.queryByText("disk")).toBeNull();
      expect(screen.getByText("logs")).toBeTruthy();
    });
  });

  it("inserts command text without adding Enter", async () => {
    const { onInsert } = renderDialog();
    const item = (await screen.findByText("disk")).closest("article")!;

    fireEvent.click(within(item).getByRole("button", { name: "Insert" }));

    expect(onInsert).toHaveBeenCalledWith("df -h");
  });

  it("disables insertion when there is no focused terminal", async () => {
    const { onInsert } = renderDialog({ canInsert: false });
    const item = (await screen.findByText("disk")).closest("article")!;
    const button = within(item).getByRole("button", { name: "Insert" });

    expect((button as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(button);
    expect(onInsert).not.toHaveBeenCalled();
  });

  it("creates a snippet through the backend command", async () => {
    renderDialog();
    await screen.findByText("disk");
    fireEvent.click(screen.getByRole("button", { name: "New" }));

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "kernel" } });
    fireEvent.change(screen.getByLabelText("Description"), {
      target: { value: "Kernel messages" },
    });
    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "dmesg -T" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(mockApi.saveSnippet).toHaveBeenCalledWith({
        name: "kernel",
        command: "dmesg -T",
        description: "Kernel messages",
      })
    );
  });
});
