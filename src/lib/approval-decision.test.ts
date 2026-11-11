import { describe, expect, it } from "vitest";
import { isAlreadyDecided } from "./approval-decision";

describe("isAlreadyDecided", () => {
  it("matches every status the daemon can already hold", () => {
    // `approval.rs:699-701` formats with `{:?}`, so these are the exact strings.
    for (const status of ["Approved", "Rejected", "TimedOut", "Revoked"]) {
      expect(isAlreadyDecided(`Failed to approve request: approval 1a2b already ${status}`)).toBe(true);
    }
  });

  it("also matches the message when Rust adds its Debug quoting", () => {
    // `ApprovalStatus::from(ApprovalStatus::Approved)` derives Debug, but a
    // future variant carrying data would print `Approved(...)`. The verb, not
    // the punctuation, is what identifies the case.
    expect(isAlreadyDecided("approval 1a2b already Approved(DuplicateRequest)")).toBe(true);
  });

  it("does not treat the other 400s as a decision", () => {
    // These leave the request pending. Retiring the card on any of them would
    // drop a request the operator still has to answer.
    for (const message of [
      "Failed to approve request: rewritten command is blocked by risk policy",
      "Failed to approve request: rewritten command is riskier (High) than the request (Medium); reject it and ask for a new request",
      "Failed to approve request: approval 1a2b has timed out",
      "Failed to approve request: unknown approval: 1a2b",
      "Failed to approve request: 400",
    ]) {
      expect(isAlreadyDecided(message)).toBe(false);
    }
  });

  it("does not match an unrelated word that merely starts like a status", () => {
    expect(isAlreadyDecided("approval 1a2b already Approvedly odd")).toBe(false);
    expect(isAlreadyDecided("approval 1a2b already blocked by policy")).toBe(false);
  });
});
