import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { IconButton } from "./icon-button";

// IconButton became load-bearing for SettingsMenu when its close key stopped
// being a hand-rolled <button>. What that site depends on is the square
// geometry and the fact that IconButton -- unlike Button -- does not normalise
// the glyph inside it to 16px. Both are pinned here.
const tokens = (container: HTMLElement) =>
  (container.firstElementChild?.className ?? "").split(/\s+/).filter(Boolean);

describe("IconButton", () => {
  it("is a square, not a label-sized box", () => {
    const { container } = render(<IconButton aria-label="Close" />);
    expect(tokens(container)).toContain("size-8");
  });

  it("honours the small square", () => {
    const { container } = render(<IconButton size="sm" aria-label="Close" />);
    const cls = tokens(container);
    expect(cls).toContain("size-7");
    expect(cls).not.toContain("size-8");
  });

  it("does not resize the glyph inside it", () => {
    // This is the reason the settings panel's close key is an IconButton and not
    // a Button: Button forces every child svg to `size-4` (16px), and that key's
    // X is 15px on purpose.
    const { container } = render(
      <IconButton size="sm" aria-label="Close">
        <svg width="15" height="15" />
      </IconButton>
    );
    expect(container.querySelector("svg")?.getAttribute("width")).toBe("15");
    expect(tokens(container)).not.toContain("[&_svg]:size-4");
  });

  it("defaults to type=button", () => {
    const { container } = render(<IconButton aria-label="Close" />);
    expect(container.firstElementChild?.getAttribute("type")).toBe("button");
  });

  it("keeps a caller's extra classes", () => {
    // `shrink-0` is what the close key passes so it survives a long panel title.
    const { container } = render(<IconButton size="sm" className="shrink-0" aria-label="Close" />);
    expect(tokens(container)).toContain("shrink-0");
  });

  it("uses the muted tone by default and the danger tone on request", () => {
    const { container: plain } = render(<IconButton aria-label="Close" />);
    expect(tokens(plain)).toContain("text-muted-foreground");
    expect(tokens(plain)).toContain("border-border");

    const { container: danger } = render(<IconButton variant="danger" aria-label="Delete" />);
    expect(tokens(danger)).toContain("hover:text-destructive");
  });
});
