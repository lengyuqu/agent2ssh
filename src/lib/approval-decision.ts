/**
 * Whether a failed `/approvals/{id}/approve|reject` means the daemon already
 * holds a decision for that request.
 *
 * The two routes answer 400 for every refusal (`bin/agent2ssh-daemon.rs:3384`,
 * `:3398`), so the status code says nothing; the message does. Only
 * `approval_respond_inner`'s "already <Status>" line (`approval.rs:699-701`)
 * means "decided", and only that one lets the caller treat the request as
 * finished. A blocked rewrite, a lost timeout or `unknown approval: <id>` all
 * keep the request pending as far as the queue is concerned, so they must not
 * match here -- over-matching would hide a request the operator still has to
 * act on.
 *
 * The status is matched against the four non-pending `ApprovalStatus` variants
 * rather than accepting any `\{debug[^}]*\}` tail, because a future message
 * such as `already blocked by policy` would otherwise retire a live card.
 */
export function isAlreadyDecided(message: string): boolean {
  return /already (Approved|Rejected|TimedOut|Revoked)\b/.test(message);
}
