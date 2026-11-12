import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import type { AgentEvent } from "../types";
import LiveActivityPanel from "./LiveActivityPanel";

const bus = vi.hoisted(() => ({ listener: undefined as ((event: AgentEvent) => void) | undefined }));
vi.mock("../eventsBus", () => ({
  useAgentEvents: (listener: (event: AgentEvent) => void) => { bus.listener = listener; },
  useEventsStatus: () => "live",
}));
vi.mock("../i18n", () => ({ useI18n: () => ({ t: (text: string) => text }) }));
vi.mock("../api", () => ({ api: { listAudit: vi.fn().mockResolvedValue([]) } }));

it("hides old-daemon PTY content even in expanded event details", async () => {
  render(<LiveActivityPanel />);
  act(() => bus.listener?.({ id: "evt", event_type: "session_input", timestamp: new Date().toISOString(),
    data: { source: "daemon", session_id: "12345678-abcd", input_bytes: 12,
      input_preview: "synthetic-pty-secret", unexpected: "synthetic-pty-secret" } }));
  await screen.findByRole("button", { name: "Expand details" });
  fireEvent.click(screen.getByRole("button", { name: "Expand details" }));
  expect(screen.getByText(/input_bytes/)).toBeTruthy();
  expect(document.body.textContent).not.toContain("synthetic-pty-secret");
});
