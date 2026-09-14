import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import { ConfirmDialog } from "./dialog";

// ConfirmDialog resolves its button labels through useI18n, and I18nProvider
// syncs tray labels over the Tauri bridge on mount. Stub the api module so
// these tests never reach Tauri internals.
vi.mock("../../api", () => ({
  api: { setTrayLabels: vi.fn().mockResolvedValue(undefined) },
}));

function renderConfirm(overrides: Partial<Parameters<typeof ConfirmDialog>[0]> = {}) {
  const props = {
    title: "Delete host web-01?",
    onConfirm: vi.fn(),
    onCancel: vi.fn(),
    ...overrides,
  };
  const view = render(
    <I18nProvider>
      <ConfirmDialog {...props} />
    </I18nProvider>
  );
  return { ...view, props };
}

function cardOf(container: HTMLElement): HTMLElement {
  const card = container.querySelector<HTMLElement>(".max-w-sm");
  if (!card) throw new Error("ConfirmDialog did not render its card");
  return card;
}

describe("ConfirmDialog", () => {
  it("shows the title and both default labels", () => {
    renderConfirm();
    expect(screen.getByText("Delete host web-01?")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Confirm" })).toBeTruthy();
  });

  it("renders the note above the buttons, where the consequence belongs", () => {
    renderConfirm({ note: <p>Open sessions become orphaned.</p> });
    const note = screen.getByText("Open sessions become orphaned.");
    const confirm = screen.getByRole("button", { name: "Confirm" });
    expect(note.compareDocumentPosition(confirm) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("adds no note layer when there is no note", () => {
    const { container } = renderConfirm();
    // Title is a <p>; the only wrapper <div> left should be the button row.
    expect(cardOf(container).querySelectorAll(":scope > div").length).toBe(1);
  });

  it("keeps the confirm button disabled while the action is in flight", () => {
    renderConfirm({ confirmLabel: "Delete", confirmDisabled: true });
    expect((screen.getByRole("button", { name: "Delete" }) as HTMLButtonElement).disabled).toBe(
      true
    );
  });

  it("uses the destructive variant only when asked", () => {
    renderConfirm({ confirmLabel: "Delete", danger: true });
    expect(screen.getByRole("button", { name: "Delete" }).className).toContain("bg-destructive");
  });

  it("stays on the default variant when danger is not asked for", () => {
    const { container } = renderConfirm({ confirmLabel: "Apply template" });
    expect(cardOf(container).querySelector(".bg-destructive")).toBeNull();
  });

  it("keeps confirm buttons at the app's default size, not the compact one", () => {
    renderConfirm({ confirmLabel: "Delete" });
    const button = screen.getByRole("button", { name: "Delete" });
    expect(button.className).toContain("h-9");
    expect(button.className).not.toContain("h-8");
  });

  it("lets a caller override either label", () => {
    renderConfirm({ confirmLabel: "Restore", cancelLabel: "Keep it" });
    expect(screen.getByRole("button", { name: "Restore" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Keep it" })).toBeTruthy();
  });

  it("reports both outcomes", () => {
    const { props } = renderConfirm({ confirmLabel: "Delete" });
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(props.onConfirm).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(props.onCancel).toHaveBeenCalledTimes(1);
  });
});
