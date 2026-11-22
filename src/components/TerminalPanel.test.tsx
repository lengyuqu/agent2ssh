import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../api";
import { I18nProvider } from "../i18n";
import { ThemeProvider } from "../theme";
import type { ConnectionStatus, HostGroup, HostProfile } from "../types";
import TerminalPanel from "./TerminalPanel";
import { ToastProvider } from "./ui/toast";

/**
 * The merged terminal surface: the host tree, the session strip, the panes and
 * the file drawer are one connection model. These tests pin the wiring that the
 * old separate workspace used to own — a tree click never piles up sessions,
 * the drawer always belongs to the focused pane's host, and a session that is
 * off screen stays alive.
 */
const registry = vi.hoisted(() => ({
  terminals: [] as Array<{
    host: string;
    props: Record<string, (...args: never[]) => unknown>;
    sent: string[];
    focusCount: number;
  }>,
}));

vi.mock("../api", () => ({
  api: {
    setTrayLabels: vi.fn(),
    listSnippets: vi.fn(),
    terminalBroadcastPreview: vi.fn(),
    terminalBroadcastRun: vi.fn(),
  },
  reportError: vi.fn(),
}));

vi.mock("./TerminalView", async () => {
  const { forwardRef, useEffect, useImperativeHandle, useRef } = await import("react");
  type StubProps = (typeof registry.terminals)[number]["props"];
  return {
    default: forwardRef<unknown, StubProps>(function TerminalViewStub(props, ref) {
      const host = props.host as unknown as string;
      const record = useRef<(typeof registry.terminals)[number] | null>(null);
      if (!record.current) record.current = { host, props, sent: [], focusCount: 0 };
      const live = record.current;
      live.host = host;
      live.props = props;
      useEffect(() => {
        registry.terminals.push(live);
        return () => {
          const index = registry.terminals.indexOf(live);
          if (index >= 0) registry.terminals.splice(index, 1);
        };
      }, []);
      useImperativeHandle(
        ref,
        () => ({
          sendText: (text: string) => live.sent.push(text),
          focus: () => {
            live.focusCount += 1;
          },
          getSelection: () => "",
          getBlocks: () => [],
          searchBlocks: () => [],
          selectBlock: () => true,
          copyBlock: async () => ({ ok: true, characters: 1 }),
          copyBlockAsImage: async () => ({ ok: true }),
          foldBlock: () => true,
          unfoldBlock: () => true,
          isBlockFolded: () => false,
        }),
        [],
      );
      return <div data-testid="terminal" data-host={host} />;
    }),
  };
});

vi.mock("./SFTPPanel", () => ({
  default: ({
    hosts,
    initialHost,
    embedded,
  }: {
    hosts: HostProfile[];
    initialHost?: string;
    embedded?: boolean;
  }) => (
    <div
      data-testid="sftp"
      data-hosts={hosts.map((host) => host.name).join(",")}
      data-initial={initialHost ?? ""}
      data-embedded={embedded ? "true" : "false"}
    />
  ),
}));

vi.mock("./SnippetsDialog", () => ({
  default: ({
    open,
    canInsert,
    onInsert,
  }: {
    open: boolean;
    canInsert: boolean;
    onInsert: (command: string) => void;
  }) => (
    <div data-testid="snippets" data-open={String(open)} data-can-insert={String(canInsert)}>
      {open && (
        <button type="button" onClick={() => onInsert("uptime")}>
          insert-snippet
        </button>
      )}
    </div>
  ),
}));

const groups: HostGroup[] = [{ id: "default", name: "Default", color: "#4A6CF7", sort_order: 0 }];

function host(name: string): HostProfile {
  return { name, host: `${name}.example.test`, user: "tester", port: 22, group: "default" };
}

const hosts = [host("zos"), host("db")];

function renderPanel(props: Partial<Parameters<typeof TerminalPanel>[0]> = {}) {
  return render(
    <I18nProvider>
      <ThemeProvider>
        <ToastProvider>
          <TerminalPanel hosts={hosts} groups={groups} connectionStatuses={[]} {...props} />
        </ToastProvider>
      </ThemeProvider>
    </I18nProvider>,
  );
}

function hostRow(name: string) {
  return screen.getByTitle(new RegExp(`^${name} · `));
}

function openHost(name: string) {
  fireEvent.click(hostRow(name));
}

function terminals() {
  return screen.getAllByTestId("terminal");
}

function visibleTerminals() {
  return terminals().filter((node) => node.parentElement?.style.visibility !== "hidden");
}

function drawers() {
  return screen.getAllByTestId("sftp");
}

function visibleDrawer() {
  return drawers().find((node) => node.parentElement?.style.display !== "none");
}

/** The height-bearing drawer box, two levels above the SFTP panel itself. */
function drawerBox() {
  return visibleDrawer()?.parentElement?.parentElement as HTMLElement;
}

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  registry.terminals.length = 0;
  // jsdom ships no matchMedia; the panel reads the OS scheme to resolve the
  // "app" terminal theme.
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    configurable: true,
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    }),
  });
  vi.mocked(api.setTrayLabels).mockResolvedValue(undefined);
  vi.mocked(api.listSnippets).mockResolvedValue([]);
});

describe("host tree → sessions", () => {
  it("opens a session and pairs the file drawer with that host alone", () => {
    renderPanel();
    expect(screen.queryByTestId("terminal")).toBeNull();
    openHost("zos");
    expect(terminals().map((node) => node.dataset.host)).toEqual(["zos"]);
    expect(visibleDrawer()?.dataset.hosts).toBe("zos");
    expect(visibleDrawer()?.dataset.initial).toBe("zos");
    // The tree row carries the open-session count.
    expect(within(hostRow("zos")).getByText("1")).toBeTruthy();
  });

  it("focuses the existing session on a repeat click and opens another from the row's +", () => {
    renderPanel();
    openHost("zos");
    openHost("zos");
    expect(terminals()).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "New session to zos" }));
    expect(terminals()).toHaveLength(2);
    expect(screen.getByTitle("zos #1")).toBeTruthy();
    expect(screen.getByTitle("zos #2")).toBeTruthy();
    // The newest session is the one on screen and the one the drawer follows.
    expect(visibleTerminals()).toHaveLength(1);
    expect(registry.terminals[1].props.host).toBe("zos");
  });

  it("opens a session to the console's selected host from the strip's +", () => {
    renderPanel({ initialHost: "db" });
    fireEvent.click(screen.getByRole("button", { name: "New terminal" }));
    expect(terminals().map((node) => node.dataset.host)).toEqual(["db"]);
  });
});

describe("file drawer", () => {
  it("follows the focused session's host and keeps the other browse state mounted", () => {
    renderPanel();
    openHost("zos");
    openHost("db");
    expect(visibleDrawer()?.dataset.hosts).toBe("db");
    fireEvent.click(screen.getByTitle("zos"));
    expect(visibleDrawer()?.dataset.hosts).toBe("zos");
    // Both panels stay mounted; only the focused one is displayed.
    expect(drawers().map((node) => node.dataset.hosts)).toEqual(["zos", "db"]);
    expect(drawers().filter((node) => node.parentElement?.style.display === "none")).toHaveLength(1);
  });

  it("hides and restores the drawer from the strip", () => {
    renderPanel();
    openHost("zos");
    fireEvent.click(screen.getByRole("button", { name: "Hide files" }));
    expect(screen.queryByTestId("sftp")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Show files" }));
    expect(visibleDrawer()?.dataset.hosts).toBe("zos");
  });

  it("fills the drawer with an embedded panel and resizes it from the divider", () => {
    renderPanel();
    openHost("zos");
    expect(visibleDrawer()?.dataset.embedded).toBe("true");
    expect(drawerBox().style.height).toBe("45%");
    const rects = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
      top: 0,
      bottom: 800,
      left: 0,
      right: 1000,
      width: 1000,
      height: 800,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    } as DOMRect);
    const divider = screen.getByRole("separator", { name: "Resize file drawer" });
    fireEvent.pointerDown(divider, { clientY: 440 });
    // jsdom has no PointerEvent; the handler only reads clientY.
    fireEvent(window, new MouseEvent("pointermove", { clientY: 320 }));
    expect(drawerBox().style.height).toBe("60%");
    fireEvent(window, new MouseEvent("pointermove", { clientY: 795 }));
    expect(drawerBox().style.height).toBe("25%"); // clamped, not collapsed
    fireEvent(window, new MouseEvent("pointerup", {}));
    fireEvent(window, new MouseEvent("pointermove", { clientY: 100 }));
    expect(drawerBox().style.height).toBe("25%"); // listeners are gone after release
    rects.mockRestore();
    const saved = JSON.parse(localStorage.getItem("agent2ssh.terminal.state") ?? "{}");
    expect(saved.filesRatio).toBe(0.25);
  });

  it("prunes the sessions whose host was removed from the configuration", () => {
    const view = renderPanel();
    openHost("zos");
    view.rerender(
      <I18nProvider>
        <ThemeProvider>
          <ToastProvider>
            <TerminalPanel hosts={[host("db")]} groups={groups} connectionStatuses={[]} />
          </ToastProvider>
        </ThemeProvider>
      </I18nProvider>,
    );
    // The session is pruned with its host, so the drawer goes with it.
    expect(screen.queryByTestId("terminal")).toBeNull();
    expect(screen.queryByTestId("sftp")).toBeNull();
    expect(screen.getByText("No active sessions")).toBeTruthy();
  });
});

describe("empty surface", () => {
  it("launches a session from the host shortcuts instead of an empty pane", () => {
    renderPanel();
    // No pane chrome until there is something to put in it.
    expect(screen.queryByLabelText("Pane 1 session")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /db\.example\.test/ }));
    expect(terminals().map((node) => node.dataset.host)).toEqual(["db"]);
    expect(screen.getByLabelText("Pane 1 session")).toBeTruthy();
    expect(screen.queryByText("Open a session")).toBeNull();
  });

  it("sends a first-time user to the host form when there is nothing to open", () => {
    const onNavigateModule = vi.fn();
    renderPanel({ hosts: [], onNavigateModule });
    fireEvent.click(screen.getByRole("button", { name: "New host" }));
    expect(onNavigateModule).toHaveBeenCalledWith("hosts");
  });
});

describe("panes and focus", () => {
  it("keeps off-screen sessions alive when the layout shows fewer panes", () => {
    renderPanel();
    openHost("zos");
    openHost("db");
    fireEvent.click(screen.getByTitle("Split horizontally"));
    const saved = JSON.parse(localStorage.getItem("agent2ssh.terminal.state") ?? "{}");
    const zosId = saved.sessions.find((session: { host: string }) => session.host === "zos").id;
    fireEvent.change(screen.getByLabelText("Pane 2 session"), { target: { value: zosId } });
    expect(terminals()).toHaveLength(2);
    expect(visibleTerminals()).toHaveLength(2);
    fireEvent.click(screen.getByTitle("Single pane"));
    expect(terminals()).toHaveLength(2);
    expect(visibleTerminals()).toHaveLength(1);
    // The hidden session's terminal is still mounted, so its shell keeps running.
    expect(registry.terminals.map((entry) => entry.host).sort()).toEqual(["db", "zos"]);
  });

  it("moves focus to the neighbour when the focused session closes", () => {
    renderPanel();
    openHost("zos");
    openHost("db");
    fireEvent.click(screen.getByRole("button", { name: "Close session to db" }));
    expect(terminals().map((node) => node.dataset.host)).toEqual(["zos"]);
    expect(visibleDrawer()?.dataset.hosts).toBe("zos");
    expect(screen.queryByTitle("db")).toBeNull();
  });

  it("hides the host column in focus mode while the strip actions stay reachable", () => {
    renderPanel();
    openHost("zos");
    expect(screen.getByLabelText("Search hosts")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Focus mode" }));
    expect(screen.queryByLabelText("Search hosts")).toBeNull();
    expect(terminals()).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Hide files" }));
    expect(screen.queryByTestId("sftp")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Focus mode" }));
    expect(screen.getByLabelText("Search hosts")).toBeTruthy();
  });

  it("shows the working directory a session reported over OSC 7", () => {
    renderPanel();
    openHost("zos");
    openHost("db");
    act(() => {
      registry.terminals[0].props.onOsc7Cwd?.("/srv/app" as never);
    });
    expect(screen.getByTitle("/srv/app").textContent).toBe("/srv/app");
    // The chip belongs to the session that reported it, not to the focused one.
    fireEvent.click(screen.getByTitle("zos"));
    expect(screen.getByTitle("/srv/app")).toBeTruthy();
  });

  it("opens the history search inside the pane that asked for it", () => {
    renderPanel();
    openHost("zos");
    openHost("db");
    fireEvent.click(screen.getByTitle("Split into 4"));
    const saved = JSON.parse(localStorage.getItem("agent2ssh.terminal.state") ?? "{}");
    const zosId = saved.sessions.find((session: { host: string }) => session.host === "zos").id;
    fireEvent.change(screen.getByLabelText("Pane 4 session"), { target: { value: zosId } });
    fireEvent.click(screen.getAllByTitle("Search command history")[3]);
    const popover = screen.getByPlaceholderText("Search command history").closest("div.absolute");
    // The bottom-right quadrant, below its own pane header — not the stage top.
    expect((popover as HTMLElement).style.top).toBe("calc(50% + 34px)");
    expect((popover as HTMLElement).style.left).toBe("calc(50% + 8px)");
    fireEvent.keyDown(screen.getByPlaceholderText("Search command history"), { key: "Escape" });
    expect(screen.queryByPlaceholderText("Search command history")).toBeNull();
  });
});

describe("session tools", () => {
  it("inserts a snippet into the focused pane", () => {
    renderPanel();
    openHost("zos");
    openHost("db");
    fireEvent.click(screen.getByTitle("zos"));
    fireEvent.click(screen.getByRole("button", { name: "Command snippets" }));
    expect(screen.getByTestId("snippets").dataset.canInsert).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "insert-snippet" }));
    expect(registry.terminals[0].sent).toEqual(["uptime"]);
    expect(registry.terminals[0].focusCount).toBe(1);
    expect(registry.terminals[1].sent).toEqual([]);
    expect(screen.getByTestId("snippets").dataset.open).toBe("false");
  });

  it("offers broadcast only once two sessions are connected, and targets those sessions", () => {
    renderPanel();
    openHost("zos");
    openHost("db");
    expect(
      screen.getByRole<HTMLButtonElement>("button", { name: "Broadcast command" }).disabled,
    ).toBe(true);
    act(() => {
      registry.terminals[0].props.onConnectionChange?.({
        terminalId: "t-zos",
        host: "zos",
      } as never);
      registry.terminals[1].props.onConnectionChange?.({ terminalId: "t-db", host: "db" } as never);
    });
    const broadcast = screen.getByRole<HTMLButtonElement>("button", { name: "Broadcast command" });
    expect(broadcast.disabled).toBe(false);
    fireEvent.click(broadcast);
    const dialog = screen
      .getByRole("heading", { name: "Broadcast command" })
      .closest(".bg-popover") as HTMLElement;
    expect(within(dialog).getByText("zos")).toBeTruthy();
    expect(within(dialog).getByText("db")).toBeTruthy();
    expect(within(dialog).getAllByRole("checkbox")).toHaveLength(3); // two targets + force
  });
});

describe("persistence", () => {
  it("restores the saved sessions, panes and layout on mount", () => {
    localStorage.setItem(
      "agent2ssh.terminal.state",
      JSON.stringify({
        sessions: [
          { id: "term-1-1", host: "zos", createdAt: 1 },
          { id: "term-2-2", host: "db", createdAt: 2 },
        ],
        panes: ["term-2-2", "term-1-1"],
        layout: "row-2",
        focusedPane: 1,
        ratios: { col: 0.4, row: 0.5 },
        filesOpen: true,
      }),
    );
    renderPanel();
    expect(terminals()).toHaveLength(2);
    expect(visibleTerminals()).toHaveLength(2);
    // Focus was restored to pane 1, which shows zos — so does the drawer.
    expect(visibleDrawer()?.dataset.hosts).toBe("zos");
    expect(screen.getByTitle("Split horizontally").className).toContain("bg-primary/10");
  });

  it("saves the surface as it changes", () => {
    renderPanel();
    openHost("zos");
    const saved = JSON.parse(localStorage.getItem("agent2ssh.terminal.state") ?? "{}");
    expect(saved.sessions.map((session: { host: string }) => session.host)).toEqual(["zos"]);
    expect(saved.panes).toEqual([saved.sessions[0].id]);
    expect(saved.filesOpen).toBe(true);
  });
});

describe("connection status", () => {
  it("marks a session connected from the host's live status", () => {
    const statuses: ConnectionStatus[] = [{ host: "zos", connected: true }];
    const view = renderPanel({ connectionStatuses: statuses });
    openHost("zos");
    openHost("db");
    expect(screen.getByTitle("zos").querySelector(".bg-success")).toBeTruthy();
    expect(screen.getByTitle("db").querySelector(".bg-success")).toBeNull();
    // A status change repaints the strip without touching the sessions.
    view.rerender(
      <I18nProvider>
        <ThemeProvider>
          <ToastProvider>
            <TerminalPanel
              hosts={hosts}
              groups={groups}
              connectionStatuses={[...statuses, { host: "db", connected: true }]}
            />
          </ToastProvider>
        </ThemeProvider>
      </I18nProvider>,
    );
    expect(screen.getByTitle("db").querySelector(".bg-success")).toBeTruthy();
    expect(terminals()).toHaveLength(2);
  });
});