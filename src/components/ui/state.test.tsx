import { render, screen } from "@testing-library/react";
import { ShieldAlert } from "lucide-react";
import { describe, expect, it } from "vitest";
import { InlineAlert, LoadingState, Spinner } from "./state";

// Spinner and InlineAlert exist to be the single source of truth for two visual
// specs that had been copied by hand across a dozen components. The contract
// callers rely on is which semantic token each variant resolves to, so that is
// what these assert — not incidental spacing.
describe("Spinner", () => {
  it("renders the mark at the requested size and spins", () => {
    const { container } = render(<Spinner size={18} />);
    const icon = container.querySelector("svg");
    expect(icon).not.toBeNull();
    expect(icon?.getAttribute("class")).toContain("animate-spin");
    expect(icon?.getAttribute("width")).toBe("18");
  });

  it("is decorative for assistive tech", () => {
    const { container } = render(<Spinner />);
    expect(container.querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
  });
});

describe("LoadingState", () => {
  it("uses the shared spinner beside its label", () => {
    const { container } = render(<LoadingState label="Loading things" />);
    expect(container.querySelector("svg")?.getAttribute("class")).toContain("animate-spin");
    expect(screen.getByText("Loading things")).toBeTruthy();
  });
});

describe("InlineAlert", () => {
  it("defaults to the warning tone, unbordered", () => {
    const { container } = render(<InlineAlert>heads up</InlineAlert>);
    const className = container.firstElementChild?.className ?? "";
    expect(className).toContain("bg-warning/10");
    expect(className).not.toContain("border");
  });

  it("switches to the destructive tone", () => {
    const { container } = render(<InlineAlert tone="destructive">failed</InlineAlert>);
    const className = container.firstElementChild?.className ?? "";
    expect(className).toContain("bg-destructive/10");
    expect(className).not.toContain("warning");
  });

  it("adds the shared border and padding when bordered", () => {
    const { container } = render(
      <InlineAlert tone="destructive" bordered>
        failed
      </InlineAlert>
    );
    const className = container.firstElementChild?.className ?? "";
    expect(className).toContain("border-destructive/30");
    expect(className).toContain("px-3");
  });

  it("puts an icon in the same row as the copy", () => {
    const { container } = render(
      <InlineAlert bordered icon={ShieldAlert}>
        watch out
      </InlineAlert>
    );
    const box = container.firstElementChild;
    expect(box?.className).toContain("flex");
    expect(box?.querySelector("svg")).not.toBeNull();
  });

  it("lets the caller override the base spec", () => {
    const { container } = render(
      <InlineAlert bordered className="rounded-lg p-3">
        note
      </InlineAlert>
    );
    const className = container.firstElementChild?.className ?? "";
    // cn() runs tailwind-merge, so the later class wins over the base spec.
    expect(className).toContain("rounded-lg");
    expect(className).toContain("p-3");
    expect(className).not.toContain("rounded-md");
  });

  it("does not let long unbroken text escape the box", () => {
    const { container } = render(<InlineAlert>{"a".repeat(400)}</InlineAlert>);
    expect(container.querySelector(".break-words")).not.toBeNull();
  });
});
