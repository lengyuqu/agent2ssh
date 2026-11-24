import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import type { ApprovalRequest } from "../../types";
import { ConfirmHost } from "../ui/dialog";
import ApprovalWorkbench from "./ApprovalWorkbench";

const mockApi = vi.hoisted(() => ({
  setTrayLabels: vi.fn().mockResolvedValue(undefined),
  fetchApprovals: vi.fn().mockResolvedValue([]),
  listAudit: vi.fn().mockResolvedValue([]),
  getApprovalMode: vi.fn().mockResolvedValue("ask"),
  setApprovalMode: vi.fn().mockResolvedValue("ask"),
}));

vi.mock("../../api", () => ({
  api: mockApi,
  reportError: vi.fn(),
}));

vi.mock("../../eventsBus", () => ({
  useAgentEvents: () => {},
}));

function makeApproval(partial: Partial<ApprovalRequest> & Pick<ApprovalRequest, "id" | "host" | "command">): ApprovalRequest {
  return {
    risk_level: "medium",
    requested_at: new Date(Date.now() - 30_000).toISOString(),
    ttl_secs: 300,
    status: "pending",
    ...partial,
  };
}

describe("ApprovalWorkbench queue controls", () => {
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem("agent2ssh.language", "en");
    vi.clearAllMocks();
    mockApi.fetchApprovals.mockResolvedValue([]);
    mockApi.listAudit.mockResolvedValue([]);
    mockApi.getApprovalMode.mockResolvedValue("ask");
  });

  it("displays TTL countdowns, filters pending approvals, and batch-rejects shown cards", async () => {
    const onReject = vi.fn().mockResolvedValue(undefined);
    const items = [
      makeApproval({
        id: "a1",
        host: "prod-web",
        command: "systemctl reload nginx",
        requested_at: new Date(Date.now() - 260_000).toISOString(),
        ttl_secs: 300,
      }),
      makeApproval({
        id: "a2",
        host: "prod-web",
        command: "systemctl restart php-fpm",
      }),
      makeApproval({
        id: "a3",
        host: "staging-db",
        command: "pg_dump main",
      }),
    ];

    render(
      <I18nProvider>
        <ApprovalWorkbench
          pending={items}
          onApprove={vi.fn()}
          onReject={onReject}
          onGoExecute={vi.fn()}
        />
        <ConfirmHost />
      </I18nProvider>,
    );

    // Urgent card (<60s remaining) displays seconds countdown.
    expect(screen.getByText(/^[1-5]?\ds$/)).toBeTruthy();

    fireEvent.change(screen.getByLabelText("Filter approvals by host or command"), {
      target: { value: "prod-web" },
    });
    expect(screen.queryByText("pg_dump main")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Reject shown (2)" }));
    const confirmTitle = await screen.findByText("Reject 2 pending approvals?");
    fireEvent.click(within(confirmTitle.parentElement!).getByRole("button", { name: "Reject" }));

    await waitFor(() => expect(onReject).toHaveBeenCalledTimes(2));
    expect(onReject).toHaveBeenNthCalledWith(1, items[0]);
    expect(onReject).toHaveBeenNthCalledWith(2, items[1]);
  });
});
