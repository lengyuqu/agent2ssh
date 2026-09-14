import { render } from "@testing-library/react";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { HostProfile } from "../types";
import HostSelector from "./HostSelector";

// I18nProvider pushes tray labels on mount/language change.
vi.mock("../api", () => ({
  api: { setTrayLabels: vi.fn().mockResolvedValue(undefined) },
  reportError: vi.fn(),
}));

function host(name: string, overrides: Partial<HostProfile> = {}): HostProfile {
  return { name, host: `${name}.internal`, group: "default", user: "deploy", port: 22, ...overrides };
}

const HOSTS = [host("alpha"), host("beta", { port: 2222 })];

// Four panels used to hand-roll this select and disagreed on the part that
// matters: whether an empty value is an answer. `allowEmpty` is that
// distinction, and it decides two observable things — whether the control
// defaults to the first host, and whether an empty host list disables it — so
// those are what these assert.
function renderSelector(props: Partial<ComponentProps<typeof HostSelector>> = {}) {
  const onChange = vi.fn();
  const view = render(
    <I18nProvider>
      <HostSelector hosts={HOSTS} value="" onChange={onChange} {...props} />
    </I18nProvider>
  );
  return { onChange, ...view };
}

/** Every option as a `[value, text]` pair, in document order. */
function optionsOf(container: HTMLElement): Array<[string, string]> {
  return Array.from(container.querySelectorAll("option")).map((option) => [
    option.value,
    option.textContent ?? "",
  ]);
}

describe("HostSelector option text", () => {
  it("names the endpoint beside each host by default", () => {
    const { container } = renderSelector();
    expect(optionsOf(container)).toEqual([
      ["alpha", "alpha - deploy@alpha.internal:22"],
      ["beta", "beta - deploy@beta.internal:2222"],
    ]);
  });

  it("lets the caller shorten it", () => {
    const { container } = renderSelector({ optionLabel: (h) => h.name });
    expect(optionsOf(container)).toEqual([
      ["alpha", "alpha"],
      ["beta", "beta"],
    ]);
  });
});

describe("HostSelector blank entry", () => {
  it("offers the caller's blank entry alongside the hosts", () => {
    const { container } = renderSelector({ emptyOption: "None" });
    expect(optionsOf(container)).toContainEqual(["", "None"]);
  });

  it("shows a placeholder instead when there is nothing to pick", () => {
    const { container } = renderSelector({ hosts: [] });
    expect(optionsOf(container)).toHaveLength(1);
    expect(optionsOf(container)[0][0]).toBe("");
  });

  it("drops that placeholder once there are hosts", () => {
    const { container } = renderSelector();
    expect(optionsOf(container).some(([value]) => value === "")).toBe(false);
  });
});

describe("HostSelector allowEmpty", () => {
  it("defaults to the first host, so the control never holds nothing", () => {
    const { onChange } = renderSelector();
    expect(onChange).toHaveBeenCalledWith("alpha");
  });

  it("leaves an empty value alone when the caller offers none", () => {
    const { onChange } = renderSelector({ allowEmpty: true });
    expect(onChange).not.toHaveBeenCalled();
  });

  it("replaces a host that is no longer selectable", () => {
    const { onChange } = renderSelector({ value: "gone" });
    expect(onChange).toHaveBeenCalledWith("alpha");
  });

  it("clears it instead, when an empty value is legal", () => {
    const { onChange } = renderSelector({ value: "gone", allowEmpty: true });
    expect(onChange).toHaveBeenCalledWith("");
  });

  it("disables itself with no hosts", () => {
    const { container } = renderSelector({ hosts: [] });
    expect(container.querySelector("select")?.disabled).toBe(true);
  });

  it("stays usable with no hosts when an empty value is legal", () => {
    const { container } = renderSelector({ hosts: [], allowEmpty: true, emptyOption: "Direct" });
    expect(container.querySelector("select")?.disabled).toBe(false);
  });
});

describe("HostSelector shell", () => {
  it("frames itself with a label and a Server mark by default", () => {
    const { container } = renderSelector({ label: "Jump host" });
    const label = container.querySelector("label");
    expect(label).not.toBeNull();
    expect(label?.textContent).toContain("Jump host");
    expect(label?.querySelector("svg")).not.toBeNull();
  });

  it("keeps a plain label without the mark", () => {
    const { container } = renderSelector({ label: "Jump host", shell: "plain" });
    const label = container.querySelector("label");
    expect(label?.textContent).toContain("Jump host");
    expect(label?.querySelector("svg")).toBeNull();
  });

  it("renders just the select in the bare shell", () => {
    const { container } = renderSelector({ shell: "bare" });
    expect(container.querySelector("label")).toBeNull();
    expect(container.querySelector("select")).not.toBeNull();
  });

  it("forwards the select class so toolbars can size it", () => {
    const { container } = renderSelector({ shell: "bare", selectClassName: "h-8 text-xs" });
    const className = container.querySelector("select")?.className ?? "";
    expect(className).toContain("h-8");
    expect(className).toContain("text-xs");
  });
});
