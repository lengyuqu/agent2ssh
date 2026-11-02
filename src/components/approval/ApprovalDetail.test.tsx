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
describe("ApprovalDetail unclassified risk", () => {
  const warning =
    "This host runs a shell the risk classifier cannot read (cmd.exe or PowerShell), so the level above may understate the real risk. Judge the command itself.";

  function withRisk(riskUnclassified: boolean): ApprovalRequest {
    const approval = request(true);
    return {
      ...approval,
      context: {
        rewritable: true,
        risk_details: {
          built_in_risk: "low",
          final_risk: "low",
          risk_unclassified: riskUnclassified,
        },
      },
    };
  }

  it("says the level is a bash verdict when the shell is not bash", () => {
    renderDetail(withRisk(true));
    expect(screen.getByText(warning)).toBeTruthy();
  });

  it("stays quiet for a shell the classifier can read", () => {
    renderDetail(withRisk(false));
    expect(screen.queryByText(warning)).toBeNull();
  });
});
