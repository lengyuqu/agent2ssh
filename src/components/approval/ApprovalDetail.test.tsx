import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import type { ApprovalRequest, RiskLevel } from "../../types";
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

  it("falls back to copy-and-open when the request cannot be rewritten", () => {
    renderDetail(request(false));
    expect(screen.queryByText("Edit then execute")).toBeNull();
    expect(screen.getByText("Copy command and open Execution")).toBeTruthy();
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

/**
 * The four context rows are the approver's only view of "who asked, which
 * policy matched, why, and under which change id" -- and until this block
 * existed, deleting the `candidateRows` computation in ApprovalDetail left all
 * six tests above green. `api.yaml` lists all four fields in `ApprovalResponse`,
 * so the contract promised them while the UI could drop them silently.
 */
describe("ApprovalDetail request context", () => {
  function withContext(context: ApprovalRequest["context"]): ApprovalRequest {
    return { ...request(true), context };
  }

  beforeEach(() => localStorage.clear());

  it("shows who asked, which policy matched, why, and the change id", () => {
    renderDetail(
      withContext({
        rewritable: true,
        source: "mcp",
        reason: "Deploys are gated",
        change_id: "CHG-42",
        risk_details: {
          built_in_risk: "high",
          final_risk: "high",
          matched_policy: "deploy-gate",
        },
      })
    );
    for (const label of ["Requested by", "Matched policy", "Reason", "Change ID"]) {
      expect(screen.getByText(label), `missing row label ${label}`).toBeTruthy();
    }
    for (const value of ["mcp", "deploy-gate", "Deploys are gated", "CHG-42"]) {
      expect(screen.getByText(value), `missing row value ${value}`).toBeTruthy();
    }
  });

  it("omits the block entirely when every field is empty", () => {
    // `contextRows.length > 0` guards the whole <dl>; without this the guard
    // could be deleted and an empty bordered box would appear instead.
    renderDetail(withContext({ rewritable: true }));
    expect(document.querySelector("dl")).toBeNull();
    expect(screen.queryByText("Requested by")).toBeNull();
  });
});

/**
 * `COMMAND_TINT` is the only place the "tint the whole row, no span-level dye"
 * decision is written down (see the component's doc comment). Replacing all
 * four values with "" left the suite green before this block existed.
 */
describe("ApprovalDetail command colour", () => {
  function withRiskLevel(level: RiskLevel): ApprovalRequest {
    return { ...request(true), risk_level: level };
  }

  it.each([
    ["low", "text-risk-low"],
    ["medium", "text-risk-medium"],
    ["high", "text-risk-high"],
    ["blocked", "text-risk-high"],
  ] as const)("paints a %s command with %s", (level, tint) => {
    localStorage.clear();
    renderDetail(withRiskLevel(level));
    const code = document.querySelector("code");
    expect(code, "the command row is not rendered").toBeTruthy();
    expect(code?.className).toContain(tint);
  });
});

