import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import HostTree from "./HostTree";

describe("HostTree", () => {
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem("agent2ssh.language", "en");
  });

  it("filters hosts by tags/role and opens a context menu with cross-module actions", () => {
    const onOpenHost = vi.fn();
    const onNewSession = vi.fn();
    const onOpenExecute = vi.fn();
    const onOpenFiles = vi.fn();
    const onInspectHost = vi.fn();

    render(
      <I18nProvider>
        <HostTree
          hosts={[
            {
              name: "prod-db",
              host: "10.0.0.5",
              user: "postgres",
              group: "default",
              role: "database",
              tags: ["critical", "eu"],
            },
            {
              name: "dev-web",
              host: "10.0.0.6",
              user: "dev",
              group: "default",
              tags: ["staging"],
            },
          ]}
          groups={[{ id: "default", name: "Default", color: "#4A6CF7", sort_order: 0 }]}
          connectionStatuses={[]}
          sessionCounts={new Map()}
          onOpenHost={onOpenHost}
          onNewSession={onNewSession}
          onOpenExecute={onOpenExecute}
          onOpenFiles={onOpenFiles}
          onInspectHost={onInspectHost}
        />
      </I18nProvider>,
    );

    const search = screen.getByLabelText("Search hosts");
    fireEvent.change(search, { target: { value: "critical" } });
    expect(screen.getByText("prod-db")).toBeTruthy();
    expect(screen.queryByText("dev-web")).toBeNull();

    fireEvent.contextMenu(screen.getByText("prod-db"), { clientX: 40, clientY: 60 });
    fireEvent.click(screen.getByRole("menuitem", { name: "Execute" }));
    expect(onOpenExecute).toHaveBeenCalledWith("prod-db");

    fireEvent.contextMenu(screen.getByText("prod-db"), { clientX: 40, clientY: 60 });
    fireEvent.click(screen.getByRole("menuitem", { name: "Files" }));
    expect(onOpenFiles).toHaveBeenCalledWith("prod-db");

    fireEvent.contextMenu(screen.getByText("prod-db"), { clientX: 40, clientY: 60 });
    fireEvent.click(screen.getByRole("menuitem", { name: "View host" }));
    expect(onInspectHost).toHaveBeenCalledWith("prod-db");
  });
});
