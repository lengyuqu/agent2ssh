import { useEffect, useState } from "react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { DockedView, TerminalDock, TerminalSlot } from "./dock";

function Probe() {
  const [mounts, setMounts] = useState(0);
  useEffect(() => {
    setMounts((count) => count + 1);
  }, []);
  return <span data-testid="probe">{mounts}</span>;
}

function Harness({ slot }: { slot: string }) {
  return (
    <TerminalDock>
      <TerminalSlot id="drawer" />
      <TerminalSlot id="main" />
      <DockedView slotId={slot}>
        <Probe />
      </DockedView>
    </TerminalDock>
  );
}

describe("terminal dock", () => {
  it("moves the view between slots without remounting it", async () => {
    const view = render(<Harness slot="drawer" />);
    const probe = await screen.findByTestId("probe");
    expect(probe.textContent).toBe("1");
    expect(document.querySelector('[data-terminal-slot="drawer"]')?.contains(probe)).toBe(true);

    view.rerender(<Harness slot="main" />);
    const moved = screen.getByTestId("probe");
    expect(moved.textContent).toBe("1");
    expect(document.querySelector('[data-terminal-slot="main"]')?.contains(moved)).toBe(true);
  });
});
