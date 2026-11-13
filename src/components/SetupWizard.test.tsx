import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import SetupWizard from "./SetupWizard";

vi.mock("../api", () => ({
  api: { setTrayLabels: vi.fn().mockResolvedValue(undefined) },
  reportError: vi.fn(),
}));

describe("SetupWizard", () => {
  it("offers a Netcatty import on the same step as ~/.ssh/config", () => {
    const onImportNetcatty = vi.fn();
    render(
      <I18nProvider>
        <SetupWizard onComplete={vi.fn()} onSkip={vi.fn()} onImportNetcatty={onImportNetcatty} />
      </I18nProvider>
    );

    fireEvent.click(screen.getByRole("button", { name: "Next" }));

    expect(
      screen.getByText(
        "Or import a Netcatty vault backup. Existing hosts are skipped, never overwritten."
      )
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Import from Netcatty" }));
    expect(onImportNetcatty).toHaveBeenCalledTimes(1);
  });
});
