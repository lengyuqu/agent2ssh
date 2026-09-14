import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { RedactRuleInfo } from "../types";
import RedactionSettings from "./RedactionSettings";

// I18nProvider syncs tray labels over the Tauri bridge on mount, and the panel
// itself mutates rules through the same bridge. Stub the whole api module so
// these tests never reach Tauri internals.
const mockApi = vi.hoisted(() => ({
  setTrayLabels: vi.fn().mockResolvedValue(undefined),
  listRedactRules: vi.fn(),
  addRedactRule: vi.fn(),
  updateRedactRule: vi.fn(),
  removeRedactRule: vi.fn(),
  resetRedactRules: vi.fn(),
}));

vi.mock("../api", () => ({
  api: mockApi,
  reportError: vi.fn(),
}));

/** The built-in IPv6 loopback rule, with a pattern short enough to assert on. */
const builtin: RedactRuleInfo = {
  pattern: "::1\\b",
  replacement: "<REDACTED:ip>",
  is_builtin: true,
};

/** A user rule that redacts by deleting the match, so `replacement` is empty. */
const custom: RedactRuleInfo = {
  pattern: "secret-project",
  replacement: "",
  is_builtin: false,
};

function renderSettings() {
  const view = render(
    <I18nProvider>
      <RedactionSettings />
    </I18nProvider>
  );
  return view;
}

/**
 * ConfirmDialog renders its card inline (no portal) at `.max-w-sm`, which is
 * what makes it distinguishable from the panel's own buttons when both carry
 * the same label.
 */
function cardOf(container: HTMLElement): HTMLElement {
  const card = container.querySelector<HTMLElement>(".max-w-sm");
  if (!card) throw new Error("no confirmation dialog is open");
  return card;
}

describe("RedactionSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockApi.setTrayLabels.mockResolvedValue(undefined);
    mockApi.listRedactRules.mockResolvedValue([builtin, custom]);
  });

  it("lists the rules, marking the built-ins and the delete-the-match replacement", async () => {
    renderSettings();

    // The panel is the only place that shows the pattern as text; the add form
    // holds it in an input value.
    await screen.findByText(builtin.pattern);
    expect(screen.getByText(custom.pattern)).toBeTruthy();
    expect(screen.getByText("<REDACTED:ip>")).toBeTruthy();

    // Exactly one rule is built in, so exactly one badge.
    expect(screen.getAllByText("Built-in")).toHaveLength(1);

    // An empty replacement is not a blank cell — it means "drop the match".
    expect(screen.getByText("(removed)")).toBeTruthy();
  });

  it("adds a rule and clears the form only once it is accepted", async () => {
    mockApi.listRedactRules.mockResolvedValue([builtin]);
    const added: RedactRuleInfo = {
      pattern: "secret-project",
      replacement: "<REDACTED:project>",
      is_builtin: false,
    };
    mockApi.addRedactRule.mockResolvedValue([builtin, added]);
    renderSettings();

    await screen.findByText(builtin.pattern);
    fireEvent.change(screen.getByPlaceholderText("Pattern"), {
      target: { value: "secret-project" },
    });
    fireEvent.change(screen.getByPlaceholderText("Replacement"), {
      target: { value: "<REDACTED:project>" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    await waitFor(() =>
      expect(mockApi.addRedactRule).toHaveBeenCalledWith("secret-project", "<REDACTED:project>")
    );
    expect((screen.getByPlaceholderText("Pattern") as HTMLInputElement).value).toBe("");
    expect(await screen.findByText("secret-project")).toBeTruthy();
  });

  it("keeps the typed rule and shows the backend message when a rule is rejected", async () => {
    mockApi.listRedactRules.mockResolvedValue([builtin]);
    mockApi.addRedactRule.mockRejectedValue("a rule for this pattern already exists: ::1\\b");
    renderSettings();

    await screen.findByText(builtin.pattern);
    fireEvent.change(screen.getByPlaceholderText("Pattern"), {
      target: { value: "already-taken" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    expect(await screen.findByText(/already exists/)).toBeTruthy();
    // The message is only useful next to the text the user needs to correct.
    expect((screen.getByPlaceholderText("Pattern") as HTMLInputElement).value).toBe(
      "already-taken"
    );
  });

  it("edits a rule in place, keyed on the pattern it started from", async () => {
    mockApi.listRedactRules.mockResolvedValue([custom]);
    const renamed: RedactRuleInfo = { ...custom, pattern: "secret-project-v2" };
    mockApi.updateRedactRule.mockResolvedValue([renamed]);
    renderSettings();

    await screen.findByText(custom.pattern);
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));

    fireEvent.change(screen.getByLabelText(`Edit pattern: ${custom.pattern}`), {
      target: { value: "secret-project-v2" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    // The original pattern identifies the rule: it is what the rename is
    // resolved against on the Rust side.
    await waitFor(() =>
      expect(mockApi.updateRedactRule).toHaveBeenCalledWith(
        "secret-project",
        "secret-project-v2",
        ""
      )
    );
    expect(await screen.findByText("secret-project-v2")).toBeTruthy();
  });

  it("does not delete a rule until the confirmation is accepted", async () => {
    mockApi.removeRedactRule.mockResolvedValue([builtin]);
    const { container } = renderSettings();

    await screen.findByText(custom.pattern);
    const row = screen.getByTitle(custom.pattern).closest("div") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: "Delete" }));

    expect(mockApi.removeRedactRule).not.toHaveBeenCalled();
    expect(screen.getByText(`Remove the ${custom.pattern} rule?`)).toBeTruthy();
    // A user rule is not built in, so no "whole class of secret" note.
    expect(screen.queryByText(/built-in rule; removing it/)).toBeNull();

    fireEvent.click(within(cardOf(container)).getByRole("button", { name: "Delete" }));

    await waitFor(() => expect(mockApi.removeRedactRule).toHaveBeenCalledWith("secret-project"));
  });

  it("spells out the consequence when the rule being deleted is a built-in", async () => {
    mockApi.removeRedactRule.mockResolvedValue([custom]);
    const { container } = renderSettings();

    await screen.findByText(builtin.pattern);
    const row = screen.getByTitle(builtin.pattern).closest("div") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: "Delete" }));

    expect(screen.getByText(/built-in rule; removing it/)).toBeTruthy();
    fireEvent.click(within(cardOf(container)).getByRole("button", { name: "Delete" }));

    await waitFor(() => expect(mockApi.removeRedactRule).toHaveBeenCalledWith("::1\\b"));
  });

  it("cancels a deletion without touching the rule set", async () => {
    const { container } = renderSettings();

    await screen.findByText(custom.pattern);
    const row = screen.getByTitle(custom.pattern).closest("div") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: "Delete" }));
    fireEvent.click(within(cardOf(container)).getByRole("button", { name: "Cancel" }));

    expect(mockApi.removeRedactRule).not.toHaveBeenCalled();
    expect(screen.queryByText(`Remove the ${custom.pattern} rule?`)).toBeNull();
  });

  it("shows an empty rule set as a warning rather than as an empty list", async () => {
    mockApi.listRedactRules.mockResolvedValue([]);
    renderSettings();

    // An empty file means nothing is redacted, which must not read as a
    // neutral "no items yet" state.
    expect(await screen.findByText(/The rule list is empty/)).toBeTruthy();
  });

  it("restores the built-in set through a confirmation", async () => {
    mockApi.listRedactRules.mockResolvedValue([]);
    mockApi.resetRedactRules.mockResolvedValue([builtin]);
    const { container } = renderSettings();

    await screen.findByText(/The rule list is empty/);
    fireEvent.click(screen.getByRole("button", { name: "Restore defaults" }));

    expect(mockApi.resetRedactRules).not.toHaveBeenCalled();
    fireEvent.click(within(cardOf(container)).getByRole("button", { name: "Restore defaults" }));

    await waitFor(() => expect(mockApi.resetRedactRules).toHaveBeenCalledTimes(1));
    expect(await screen.findByText(builtin.pattern)).toBeTruthy();
  });
});
