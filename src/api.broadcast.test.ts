import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { api } from "./api";

describe("terminal broadcast API", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue("daemon-token");
    vi.restoreAllMocks();
  });

  it("previews a frozen all-or-nothing target list", async () => {
    const response = {
      broadcast_id: "preview-id",
      enqueued_any: false,
      all_or_nothing: true,
      targets: [],
    };
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify(response), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }),
    );

    await expect(
      api.terminalBroadcastPreview({
        targets: [
          { terminal_id: "terminal-one", host: "one" },
          { terminal_id: "terminal-two", host: "two" },
        ],
        command: "uptime",
      }),
    ).resolves.toEqual(response);

    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:7722/terminal/broadcast/preview",
      expect.objectContaining({ method: "POST" }),
    );
    const request = fetchMock.mock.calls[0][1] as RequestInit;
    expect(JSON.parse(String(request.body))).toEqual({
      targets: [
        { terminal_id: "terminal-one", host: "one" },
        { terminal_id: "terminal-two", host: "two" },
      ],
      command: "uptime",
      all_or_nothing: true,
    });
  });

  it("surfaces a failed run response instead of pretending it was sent", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response("denied", { status: 403 }));
    await expect(
      api.terminalBroadcastRun({
        targets: [
          { terminal_id: "terminal-one", host: "one" },
          { terminal_id: "terminal-two", host: "two" },
        ],
        command: "sudo reboot",
      }),
    ).rejects.toThrow("Failed to run terminal broadcast: 403");
  });
});
