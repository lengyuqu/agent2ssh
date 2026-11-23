import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import HostDrawer from "./HostDrawer";
import { I18nProvider } from "../i18n";

vi.mock("../api", () => ({
  api: { setTrayLabels: vi.fn().mockResolvedValue(undefined) },
}));

function renderDrawer(
  props: Partial<{ open: boolean; editingHostName: string | null; onClose: () => void }> = {}
) {
  const onClose = props.onClose ?? vi.fn();
  const view = render(
    <I18nProvider>
      <HostDrawer
        open={props.open ?? true}
        editingHostName={props.editingHostName}
        onClose={onClose}
      >
        <div>host form</div>
      </HostDrawer>
    </I18nProvider>
  );
  return { ...view, onClose };
}

describe("HostDrawer", () => {
  it("titles an add drawer and an edit drawer differently", () => {
    const { rerender, onClose } = renderDrawer();

    expect(screen.getByRole("heading", { name: "Add Host" })).toBeTruthy();
    expect(screen.getByText("host form")).toBeTruthy();

    rerender(
      <I18nProvider>
        <HostDrawer open editingHostName="web-01" onClose={onClose}>
          <div>host form</div>
        </HostDrawer>
      </I18nProvider>
    );

    expect(screen.getByRole("heading", { name: "Edit host: web-01" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "Add Host" })).toBeNull();
  });

  it("closes when the overlay or the X button is clicked", () => {
    const { onClose } = renderDrawer();

    fireEvent.click(screen.getByRole("dialog"));
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("host-drawer-overlay"));
    expect(onClose).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByTitle("Close"));
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("closes on Escape only while open", () => {
    const onClose = vi.fn();
    const { rerender } = renderDrawer({ onClose });

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);

    rerender(
      <I18nProvider>
        <HostDrawer open={false} onClose={onClose}>
          <div>host form</div>
        </HostDrawer>
      </I18nProvider>
    );

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
