import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import type { ApprovalRequest } from "../../types";
import ApprovalDetail from "./ApprovalDetail";

vi.mock("../../api", () => ({
  api: { setTrayLabels: vi.fn().mockResolvedValue(undefined) },
  reportError: vi.fn(),
}));

function request(rewritable: boolean): ApprovalRequest {
  return {
    id: "a1",
    host: "web-1",
    command: "rm -rf /tmp/build",
    risk_level: "high",
    requested_at: "2026-10-27T10:00:00Z",
    ttl_secs: 300,
    status: "pending",
    context: { rewritable },
  };
}

function renderDetail(approval: ApprovalRequest, onApprove = vi.fn()) {
  render(
    <I18nProvider>
      <ApprovalDetail
        approval={approval}
        busy={false}
        onApprove={onApprove}
        onReject={vi.fn()}
        onCopyAndExecute={vi.fn()}
      />
    </I18nProvider>
  );
  return onApprove;
}

describe("ApprovalDetail edit then execute", () => {
  it("approves the edited command", () => {
    const onApprove = renderDetail(request(true));
    fireEvent.click(screen.getByText("Edit then execute"));
    fireEvent.change(screen.getByLabelText("Edited command"), {
      target: { value: "  ls /tmp/build  " },
    });
    fireEvent.click(screen.getByText("Approve edited command"));
    expect(onApprove).toHaveBeenCalledWith("ls /tmp/build");
  });

  it("approves as requested when the text is left unchanged", () => {
    const onApprove = renderDetail(request(true));
    fireEvent.click(screen.getByText("Edit then execute"));
    fireEvent.click(screen.getByText("Approve edited command"));
    expect(onApprove).toHaveBeenCalledWith(undefined);
  });

  it("falls back to copy-and-go when the request cannot be rewritten", () => {
    renderDetail(request(false));
    expect(screen.queryByText("Edit then execute")).toBeNull();
    expect(screen.getByText("Copy command and go to Execution")).toBeTruthy();
  });

  it("does not offer approval of an emptied command", () => {
    renderDetail(request(true));
    fireEvent.click(screen.getByText("Edit then execute"));
    fireEvent.change(screen.getByLabelText("Edited command"), { target: { value: "   " } });
    const button = screen.getByText("Approve edited command") as HTMLButtonElement;
    expect(button.disabled).toBe(true);
  });
});
