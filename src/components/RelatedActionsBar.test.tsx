import { fireEvent, render, screen } from "@testing-library/react";
import { Play, Server } from "lucide-react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider, zh } from "../i18n";
import RelatedActionsBar from "./RelatedActionsBar";

const current = { id: "hosts", label: "Host Management", icon: Server };
const related = [{ id: "execute", label: "Execution", icon: Play }];

describe("RelatedActionsBar", () => {
  it("translates the lead-in", () => {
    expect(zh["Quick jump"]).toBe("快捷穿梭");
  });

  it("renders related modules as pills and navigates by module id", () => {
    const onNavigate = vi.fn();
    const { container } = render(
      <I18nProvider>
        <RelatedActionsBar current={current} related={related} onNavigate={onNavigate} />
      </I18nProvider>,
    );

    const nav = screen.getByRole("navigation", { name: "Related modules" });
    expect(nav.getAttribute("aria-label")).toBe("Related modules");
    expect(container.querySelector(".lucide-chevron-right")).toBeNull();
    expect(screen.getByText("Quick jump")).toBeTruthy();

    const pill = screen.getByRole("button", { name: "Execution" });
    expect(pill.className).toContain("rounded-full");
    expect(pill.className).toContain("border-border");
    expect(pill.className).toContain("hover:border-primary");
    expect(pill.className).toContain("hover:text-primary");
    fireEvent.click(pill);
    expect(onNavigate).toHaveBeenCalledTimes(1);
    expect(onNavigate).toHaveBeenCalledWith("execute");
  });

  it("renders nothing when there is nowhere to jump", () => {
    const { container } = render(
      <I18nProvider>
        <RelatedActionsBar current={current} related={[]} onNavigate={vi.fn()} />
      </I18nProvider>,
    );
    expect(container.innerHTML).toBe("");
    expect(screen.queryByRole("navigation")).toBeNull();
  });
});
