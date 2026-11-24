import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { ForwardRule, ForwardRuleStats, HostProfile } from "../types";
import ForwardPanel from "./ForwardPanel";
import { ToastProvider } from "./ui/toast";

const mockApi = vi.hoisted(() => ({
  setTrayLabels: vi.fn().mockResolvedValue(undefined),
  forwardList: vi.fn(),
  forwardStats: vi.fn(),
  forwardAdd: vi.fn(),
  forwardRemove: vi.fn(),
}));

vi.mock("../api", () => ({
  api: mockApi,
  reportError: vi.fn(),
}));

function host(name: string): HostProfile {
  return { name, host: "10.0.0.1", group: "default" };
}

function rule(partial: Partial<ForwardRule> & Pick<ForwardRule, "id" | "host">): ForwardRule {
  return {
    direction: "local",
    bind_port: 8080,
    target_host: "localhost",
    target_port: 80,
    ...partial,
  };
}

function live(connections = 1): ForwardRuleStats {
  return {
    bytes_tx: 10,
    bytes_rx: 20,
    connections,
    state: "active",
    effective_port: 8080,
    connected: true,
  };
}

function renderPanel(
  props: Partial<{
    hosts: HostProfile[];
    initialHost: string;
    onSelectHost: (name: string) => void;
  }> = {}
) {
  return render(
    <I18nProvider>
      <ToastProvider>
        <ForwardPanel
          hosts={props.hosts ?? [host("web-01"), host("jump-01")]}
          initialHost={props.initialHost ?? "web-01"}
          onSelectHost={props.onSelectHost}
        />
      </ToastProvider>
    </I18nProvider>
  );
}

describe("ForwardPanel drawer", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockApi.forwardList.mockResolvedValue([]);
    mockApi.forwardStats.mockResolvedValue({});
    mockApi.forwardAdd.mockResolvedValue(rule({ id: "created", host: "web-01" }));
    mockApi.forwardRemove.mockResolvedValue(undefined);
  });

  it("opens the new-tunnel drawer and closes it after save", async () => {
    renderPanel();
    await waitFor(() => expect(mockApi.forwardList).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: "New tunnel" }));
    expect(screen.getByRole("heading", { name: "New tunnel" })).toBeTruthy();
    expect(screen.getByLabelText("Bind port")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Add tunnel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(mockApi.forwardAdd).toHaveBeenCalledWith("web-01", "local", 8080, "localhost", 80, "");
  });

  it("copies a rule into a new tunnel without removing the original", async () => {
    const existing = rule({
      id: "rule-1",
      host: "web-01",
      direction: "remote",
      bind_port: 2222,
      target_host: "db.internal",
      target_port: 5432,
      via: "jump-01",
    });
    mockApi.forwardList.mockResolvedValue([existing]);
    mockApi.forwardAdd.mockResolvedValue(rule({ id: "rule-2", host: "web-01", bind_port: 2223 }));
    renderPanel();

    fireEvent.click(await screen.findByTitle("Copy into new tunnel"));
    expect(screen.getByRole("heading", { name: "Copy tunnel" })).toBeTruthy();
    expect((screen.getByLabelText("Bind port") as HTMLInputElement).value).toBe("2222");
    expect((screen.getByLabelText("Direction") as HTMLSelectElement).value).toBe("remote");
    expect((screen.getByLabelText("Target host") as HTMLInputElement).value).toBe("db.internal");

    fireEvent.change(screen.getByLabelText("Bind port"), { target: { value: "2223" } });
    fireEvent.click(screen.getByRole("button", { name: "Add tunnel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(mockApi.forwardAdd).toHaveBeenCalledWith(
      "web-01",
      "remote",
      2223,
      "db.internal",
      5432,
      "jump-01"
    );
    expect(mockApi.forwardRemove).not.toHaveBeenCalled();
  });

  it("edits a rule on the same listener port by stopping the old listener first", async () => {
    const order: string[] = [];
    const existing = rule({
      id: "rule-1",
      host: "web-01",
      direction: "local",
      bind_port: 8080,
      target_host: "localhost",
      target_port: 80,
    });
    mockApi.forwardList.mockResolvedValue([existing]);
    mockApi.forwardRemove.mockImplementation(async () => {
      order.push("remove");
    });
    mockApi.forwardAdd.mockImplementation(async () => {
      order.push("add");
      return rule({ id: "rule-2", host: "web-01", bind_port: 8080, target_port: 3000 });
    });
    renderPanel();

    fireEvent.click(await screen.findByTitle("Edit tunnel"));
    expect(screen.getByRole("heading", { name: "Edit tunnel" })).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Target port"), { target: { value: "3000" } });
    fireEvent.click(screen.getByRole("button", { name: "Replace tunnel" }));

    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(order).toEqual(["remove", "add"]);
    expect(mockApi.forwardRemove).toHaveBeenCalledWith("rule-1");
    expect(mockApi.forwardAdd).toHaveBeenCalledWith("web-01", "local", 8080, "localhost", 3000, "");
  });

  it("restores the previous tunnel when a same-port edit fails to rebind", async () => {
    const existing = rule({
      id: "rule-1",
      host: "web-01",
      direction: "local",
      bind_port: 8080,
      target_host: "localhost",
      target_port: 80,
      via: "jump-01",
    });
    mockApi.forwardList.mockResolvedValue([existing]);
    mockApi.forwardAdd
      .mockRejectedValueOnce(new Error("target unreachable"))
      .mockResolvedValueOnce(existing);
    renderPanel();

    fireEvent.click(await screen.findByTitle("Edit tunnel"));
    fireEvent.change(screen.getByLabelText("Target port"), { target: { value: "3000" } });
    fireEvent.click(screen.getByRole("button", { name: "Replace tunnel" }));

    await waitFor(() => expect(mockApi.forwardAdd).toHaveBeenCalledTimes(2));
    expect(mockApi.forwardAdd).toHaveBeenNthCalledWith(
      2,
      "web-01",
      "local",
      8080,
      "localhost",
      80,
      "jump-01"
    );
    expect(screen.getByRole("dialog")).toBeTruthy();
  });

  it("edits a rule onto a new port by adding first and then removing the old rule", async () => {
    const order: string[] = [];
    const existing = rule({
      id: "rule-1",
      host: "web-01",
      direction: "local",
      bind_port: 8080,
      target_host: "localhost",
      target_port: 80,
    });
    mockApi.forwardList.mockResolvedValue([existing]);
    mockApi.forwardAdd.mockImplementation(async () => {
      order.push("add");
      return rule({ id: "rule-2", host: "web-01", bind_port: 9090, target_port: 80 });
    });
    mockApi.forwardRemove.mockImplementation(async () => {
      order.push("remove");
    });
    renderPanel();

    fireEvent.click(await screen.findByTitle("Edit tunnel"));
    fireEvent.change(screen.getByLabelText("Bind port"), { target: { value: "9090" } });
    fireEvent.click(screen.getByRole("button", { name: "Replace tunnel" }));

    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(order).toEqual(["add", "remove"]);
    expect(mockApi.forwardRemove).toHaveBeenCalledWith("rule-1");
  });

  it("filters the list by host and follows a host name", async () => {
    const onSelectHost = vi.fn();
    mockApi.forwardList.mockResolvedValue([
      rule({ id: "a", host: "web-01", target_host: "db.internal", target_port: 5432 }),
      rule({ id: "b", host: "web-02", target_host: "cache.internal", target_port: 11211 }),
    ]);
    renderPanel({ hosts: [host("web-01"), host("web-02")], onSelectHost });

    expect(await screen.findByText("db.internal:5432")).toBeTruthy();
    fireEvent.change(screen.getByRole("combobox", { name: "Filter by host" }), {
      target: { value: "web-02" },
    });
    expect(screen.queryByText("db.internal:5432")).toBeNull();
    expect(screen.getByText("cache.internal:11211")).toBeTruthy();

    fireEvent.change(screen.getByRole("combobox", { name: "Filter by host" }), {
      target: { value: "" },
    });
    fireEvent.click(screen.getByRole("button", { name: "web-01" }));
    expect(onSelectHost).toHaveBeenCalledWith("web-01");
  });
});

describe("ForwardPanel stats polling", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    mockApi.forwardList.mockReset();
    mockApi.forwardStats.mockReset();
    mockApi.forwardAdd.mockResolvedValue(rule({ id: "created", host: "web-01" }));
    mockApi.forwardRemove.mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.useRealTimers();
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => "visible",
    });
  });

  it("polls forwardStats every 3s only while rules exist", async () => {
    mockApi.forwardList.mockResolvedValue([]);
    mockApi.forwardStats.mockResolvedValue({});
    const empty = renderPanel();

    await waitFor(() => expect(mockApi.forwardStats).toHaveBeenCalledTimes(1));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(9000);
    });
    expect(mockApi.forwardStats).toHaveBeenCalledTimes(1);
    expect(mockApi.forwardList).toHaveBeenCalledTimes(1);
    empty.unmount();

    mockApi.forwardList.mockResolvedValue([rule({ id: "rule-1", host: "web-01" })]);
    mockApi.forwardStats.mockResolvedValue({ "rule-1": live(1) });
    const withRules = renderPanel();
    await waitFor(() => expect(mockApi.forwardStats).toHaveBeenCalledTimes(2));

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2999);
    });
    expect(mockApi.forwardStats).toHaveBeenCalledTimes(2);

    mockApi.forwardStats.mockResolvedValue({ "rule-1": live(4) });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(mockApi.forwardStats).toHaveBeenCalledTimes(3);
    expect(await screen.findByText(/4 connections/)).toBeTruthy();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(mockApi.forwardStats).toHaveBeenCalledTimes(4);
    expect(mockApi.forwardList).toHaveBeenCalledTimes(2);

    withRules.unmount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(9000);
    });
    expect(mockApi.forwardStats).toHaveBeenCalledTimes(4);
  });

  it("stays quiet when a stats tick fails and skips the tick while hidden", async () => {
    mockApi.forwardList.mockResolvedValue([rule({ id: "rule-1", host: "web-01" })]);
    mockApi.forwardStats
      .mockResolvedValueOnce({ "rule-1": live(1) })
      .mockRejectedValueOnce(new Error("stats down"))
      .mockResolvedValue({ "rule-1": live(1) });
    renderPanel();
    await waitFor(() => expect(mockApi.forwardStats).toHaveBeenCalledTimes(1));

    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(mockApi.forwardStats).toHaveBeenCalledTimes(2);
    expect(screen.queryByText("stats down")).toBeNull();

    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => "hidden",
    });
    document.dispatchEvent(new Event("visibilitychange"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(9000);
    });
    expect(mockApi.forwardStats).toHaveBeenCalledTimes(2);

    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => "visible",
    });
    document.dispatchEvent(new Event("visibilitychange"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(mockApi.forwardStats).toHaveBeenCalledTimes(3);
  });
});
