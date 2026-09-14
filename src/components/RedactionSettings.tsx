import { useEffect, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { RedactRuleInfo } from "../types";
import { Badge } from "./ui/badge";
import { ConfirmDialog } from "./ui/dialog";
import { InlineAlert, LoadingState } from "./ui/state";

/**
 * The add/edit form's shape. A rule is a pattern and a replacement; the pattern
 * is its identity, so unlike a highlight rule there is no name to edit.
 */
type Draft = { pattern: string; replacement: string };

const emptyDraft: Draft = { pattern: "", replacement: "" };

/**
 * A24: The rules the app applies to logs, audit records, notifications and the
 * diagnostic export.
 *
 * `redact_rules.json` *is* the rule set, not a copy of a built-in one:
 * `store::redact_sensitive_text` reads that file on every call, and the audit,
 * notification, playbook and diagnostic-export paths all go through it. So an
 * edit here changes what every later write redacts, in both directions —
 * adding a rule redacts something that used to be written out verbatim, and
 * removing one stops a whole class of secret being redacted everywhere the app
 * writes.
 *
 * That second direction is why deletion is not treated as an ordinary list
 * edit. It goes through a confirmation that names the consequence, built-in
 * rules carry a badge so that confirmation can be specific about what is lost,
 * and an empty set is shown as a warning rather than as a neutral "no items"
 * state: an empty rules file means the app redacts nothing at all. "Restore
 * defaults" is the way back, and it only ever adds rules.
 *
 * An earlier revision exposed only the restore, on the grounds that a rule set
 * which can only gain rules cannot be a downgrade. The asymmetry is real —
 * add and restore make redaction stricter, delete and replace make it weaker —
 * but it is not a reason to withhold the operation. The terminal highlight
 * rules are already edited in full from this same settings panel, and
 * withholding it here would have left this the only rule set in the app that
 * could only be changed by hand-editing its JSON. The guardrail is therefore
 * the confirmation and the badge rather than the absence of the operation.
 *
 * (`copy_redact::redact_for_clipboard` is a third rule file and has never had
 * an editing surface — it reads `copy_redact_rules.json` and nothing writes it
 * but the app's own seeding — so it is not the precedent to follow here.)
 *
 * There is no change event to dispatch here, unlike `HighlightSettings`: the
 * terminal keeps highlight rules in memory and has to be told when they move,
 * while redaction reads the file from Rust on every call and so is never stale.
 */
export default function RedactionSettings() {
  const { t } = useI18n();
  const [rules, setRules] = useState<RedactRuleInfo[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft>(emptyDraft);
  /** Pattern of the rule being edited in place, or null when none is. */
  const [editingPattern, setEditingPattern] = useState<string | null>(null);
  const [editDraft, setEditDraft] = useState<Draft>(emptyDraft);
  const [pendingDelete, setPendingDelete] = useState<RedactRuleInfo | null>(null);
  const [confirmingReset, setConfirmingReset] = useState(false);

  useEffect(() => {
    void api
      .listRedactRules()
      .then(setRules)
      .catch((err) => setMessage(String(err)));
  }, []);

  /** Run a mutation and adopt the list it returns. Reports whether it succeeded. */
  async function mutate(action: () => Promise<RedactRuleInfo[]>): Promise<boolean> {
    setBusy(true);
    setMessage(null);
    try {
      setRules(await action());
      return true;
    } catch (err) {
      setMessage(String(err));
      reportError("redaction-settings", "redaction rule update failed", err);
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function addRule() {
    const { pattern, replacement } = draft;
    if (!pattern.trim()) return;
    // Only clear the form when the rule was accepted. On a duplicate pattern or
    // an invalid regex the backend's message is the useful part, and the text
    // the user typed is what they need in order to correct it.
    if (await mutate(() => api.addRedactRule(pattern, replacement))) {
      setDraft(emptyDraft);
    }
  }

  function startEditing(rule: RedactRuleInfo) {
    setMessage(null);
    setEditingPattern(rule.pattern);
    setEditDraft({ pattern: rule.pattern, replacement: rule.replacement });
  }

  async function saveEdit() {
    if (editingPattern === null || !editDraft.pattern.trim()) return;
    const { pattern, replacement } = editDraft;
    // A failed save keeps the editor open with the user's text intact, so a
    // rejected pattern can be fixed rather than retyped.
    if (await mutate(() => api.updateRedactRule(editingPattern, pattern, replacement))) {
      setEditingPattern(null);
    }
  }

  async function confirmDelete() {
    const rule = pendingDelete;
    setPendingDelete(null);
    if (rule) await mutate(() => api.removeRedactRule(rule.pattern));
  }

  async function restoreDefaults() {
    setConfirmingReset(false);
    // A custom rule that was being edited may not survive the restore, so drop
    // the editor rather than leave it pointing at a pattern that is now gone.
    if (await mutate(() => api.resetRedactRules())) {
      setEditingPattern(null);
    }
  }

  if (rules === null) {
    return message ? (
      <div className="break-words text-xs text-destructive">{message}</div>
    ) : (
      <LoadingState label={t("Loading...")} />
    );
  }

  return (
    <div className="grid gap-2">
      {rules.length === 0 && (
        <InlineAlert tone="destructive" bordered>
          {t(
            "The rule list is empty, so audit records, notifications and exported diagnostics are written verbatim."
          )}
        </InlineAlert>
      )}
      <div className="grid max-h-36 gap-1.5 overflow-auto">
        {rules.map((rule) =>
          editingPattern === rule.pattern ? (
            <div
              key={rule.pattern}
              className="flex items-center gap-1.5 rounded-md border border-input bg-background px-2 py-1"
            >
              <input
                value={editDraft.pattern}
                aria-label={t("Edit pattern: {pattern}", { pattern: rule.pattern })}
                onChange={(event) => setEditDraft({ ...editDraft, pattern: event.target.value })}
                className="h-7 min-w-0 flex-1 rounded-md border border-input bg-background px-1.5 font-mono text-xs"
              />
              <input
                value={editDraft.replacement}
                aria-label={t("Edit replacement: {pattern}", { pattern: rule.pattern })}
                onChange={(event) => setEditDraft({ ...editDraft, replacement: event.target.value })}
                className="h-7 w-24 shrink-0 rounded-md border border-input bg-background px-1.5 font-mono text-xs"
              />
              <button
                type="button"
                className="shrink-0 rounded px-1.5 py-0.5 text-xs font-bold hover:bg-muted disabled:opacity-50"
                disabled={busy || !editDraft.pattern.trim()}
                onClick={() => void saveEdit()}
              >
                {t("Save")}
              </button>
              <button
                type="button"
                className="shrink-0 rounded px-1.5 py-0.5 text-xs text-muted-foreground hover:bg-muted"
                disabled={busy}
                onClick={() => setEditingPattern(null)}
              >
                {t("Cancel")}
              </button>
            </div>
          ) : (
            <div
              key={rule.pattern}
              className="flex items-center gap-2 rounded-md border border-border bg-muted/35 px-2 py-1.5"
            >
              {rule.is_builtin && (
                <Badge variant="outline" className="shrink-0">
                  {t("Built-in")}
                </Badge>
              )}
              <code className="min-w-0 flex-1 truncate font-mono text-xs" title={rule.pattern}>
                {rule.pattern}
              </code>
              <span className="shrink-0 font-mono text-xs text-muted-foreground">
                {rule.replacement || t("(removed)")}
              </span>
              <button
                type="button"
                className="shrink-0 rounded px-1.5 py-0.5 text-xs text-muted-foreground hover:bg-muted"
                disabled={busy}
                onClick={() => startEditing(rule)}
              >
                {t("Edit")}
              </button>
              <button
                type="button"
                className="shrink-0 rounded px-1.5 py-0.5 text-xs text-destructive hover:bg-destructive/10"
                disabled={busy}
                onClick={() => setPendingDelete(rule)}
              >
                {t("Delete")}
              </button>
            </div>
          )
        )}
      </div>
      <div className="grid grid-cols-[1fr_1fr_auto] gap-1.5">
        <input
          value={draft.pattern}
          onChange={(event) => setDraft({ ...draft, pattern: event.target.value })}
          placeholder={t("Pattern")}
          className="h-8 min-w-0 rounded-md border border-input bg-background px-2 font-mono text-xs"
        />
        <input
          value={draft.replacement}
          onChange={(event) => setDraft({ ...draft, replacement: event.target.value })}
          placeholder={t("Replacement")}
          className="h-8 min-w-0 rounded-md border border-input bg-background px-2 font-mono text-xs"
        />
        <button
          type="button"
          className="rounded-md bg-primary px-2 py-1 text-xs font-bold text-primary-foreground disabled:opacity-50"
          disabled={busy || !draft.pattern.trim()}
          onClick={() => void addRule()}
        >
          {t("Add")}
        </button>
      </div>
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <span className="min-w-0 flex-1">
          {t("Rules live in redact_rules.json in your config folder.")}
        </span>
        <button
          type="button"
          className="shrink-0 rounded-md border border-input bg-card px-2 py-1 text-xs font-bold hover:bg-muted disabled:opacity-50"
          disabled={busy}
          onClick={() => setConfirmingReset(true)}
        >
          {t("Restore defaults")}
        </button>
      </div>
      {message && <div className="break-words text-xs text-destructive">{message}</div>}
      {pendingDelete && (
        <ConfirmDialog
          title={t("Remove the {pattern} rule?", { pattern: pendingDelete.pattern })}
          description={t(
            "Matching text will no longer be redacted in audit records, notifications or exported diagnostics written from now on."
          )}
          note={
            pendingDelete.is_builtin ? (
              <InlineAlert tone="destructive">
                {t(
                  "This is a built-in rule; removing it stops a whole class of secret being redacted everywhere the app writes."
                )}
              </InlineAlert>
            ) : undefined
          }
          confirmLabel={t("Delete")}
          danger
          confirmDisabled={busy}
          onConfirm={() => void confirmDelete()}
          onCancel={() => setPendingDelete(null)}
        />
      )}
      {confirmingReset && (
        <ConfirmDialog
          title={t("Restore the default redaction rules?")}
          description={t(
            "This replaces the current rules with the built-in set: any you added are discarded, and any you removed come back."
          )}
          confirmLabel={t("Restore defaults")}
          confirmDisabled={busy}
          onConfirm={() => void restoreDefaults()}
          onCancel={() => setConfirmingReset(false)}
        />
      )}
    </div>
  );
}
