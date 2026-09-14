import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Button } from "./button";

// Button grew two axes that nothing used before the settings panel moved onto
// it: `block` (fill the container) and `align` (row vs. button). Both are purely
// additive -- `defaultVariants` was not touched -- so the assertions come in
// pairs: what the new axis does, and what a caller who never passes it gets.
//
// Class assertions are token-based rather than substring-based on purpose. A
// substring check for `text-foreground` also matches `hover:text-foreground`,
// which `variant="outline"` always carries, so negative assertions would be
// silently vacuous.
const tokens = (container: HTMLElement) =>
  (container.firstElementChild?.className ?? "").split(/\s+/).filter(Boolean);

describe("Button", () => {
  it("is sized by its label and centred by default", () => {
    const { container } = render(<Button>Save</Button>);
    const cls = tokens(container);
    expect(cls).toContain("h-9");
    expect(cls).toContain("px-4");
    expect(cls).toContain("justify-center");
    expect(cls).not.toContain("w-full");
    expect(cls).not.toContain("justify-start");
    expect(cls).not.toContain("text-left");
  });

  it("defaults to type=button", () => {
    // The 23 sites in SettingsMenu used to spell this out by hand.
    const { container } = render(<Button>Save</Button>);
    expect(container.firstElementChild?.getAttribute("type")).toBe("button");
  });

  it("fills its container when block is set", () => {
    const { container } = render(<Button block>Refresh gate status</Button>);
    expect(tokens(container)).toContain("w-full");
  });

  it("turns into a row when align is start", () => {
    const { container } = render(<Button align="start">Edit SSH algorithms</Button>);
    const cls = tokens(container);
    expect(cls).toContain("justify-start");
    expect(cls).toContain("text-left");
    expect(cls).not.toContain("justify-center");
  });

  it("leaves text-align alone when align is centre", () => {
    // The app imports Tailwind's theme and utilities layers but not preflight,
    // so a <button> keeps the UA's `text-align: center`. Only rows opt into
    // text-left; buttons keep whatever the browser gives them.
    const { container } = render(<Button align="center">Save</Button>);
    expect(tokens(container)).not.toContain("text-left");
  });

  it("carries the surface tokens a settings row reads against the panel", () => {
    const { container } = render(<Button variant="outline">Refresh</Button>);
    const cls = tokens(container);
    expect(cls).toContain("border-input");
    expect(cls).toContain("bg-card");
    expect(cls).toContain("text-foreground");
    expect(cls).toContain("hover:bg-muted");
  });

  it("forces child glyphs to size-4", () => {
    // The other half of the pairing with IconButton: 16px icons are the norm in
    // this app, so Button normalises them, and the one 15px icon in the settings
    // panel is why that site is an IconButton instead.
    const { container } = render(<Button>Save</Button>);
    expect(tokens(container)).toContain("[&_svg]:size-4");
  });

  it("lets a caller restate any part of the base spec", () => {
    // cn() runs tailwind-merge, which is what makes `className` an override
    // channel rather than a second list of classes. The settings panel leans on
    // all of these: h-auto for the theme swatches, px-2.5/px-2 for the row
    // paddings, font-bold for its heavier labels, /80 for the header trigger.
    const { container } = render(
      <Button
        variant="outline"
        block
        align="start"
        className="h-auto px-2.5 font-bold text-foreground/80"
      >
        Theme
      </Button>
    );
    const cls = tokens(container);
    expect(cls).toContain("h-auto");
    expect(cls).not.toContain("h-9");
    expect(cls).toContain("px-2.5");
    expect(cls).not.toContain("px-4");
    expect(cls).toContain("font-bold");
    expect(cls).not.toContain("font-semibold");
    expect(cls).toContain("text-foreground/80");
    expect(cls).toContain("w-full");
  });

  it("keeps the selected-theme tokens over the outline surface", () => {
    const { container } = render(
      <Button variant="outline" align="start" className="border-primary bg-primary/10 text-primary">
        Midnight Ops
      </Button>
    );
    const cls = tokens(container);
    expect(cls).toContain("border-primary");
    expect(cls).not.toContain("border-input");
    expect(cls).toContain("bg-primary/10");
    expect(cls).not.toContain("bg-card");
    expect(cls).toContain("text-primary");
    expect(cls).not.toContain("text-foreground");
    // The hover pair survives the merge, so a selected swatch still darkens.
    expect(cls).toContain("hover:bg-muted");
  });
});
