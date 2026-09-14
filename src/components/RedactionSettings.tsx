import { useEffect, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { RedactRuleConfig } from "../types";
import { ConfirmDialog } from "./ui/dialog";
import { LoadingState } from "./ui/state";

/**
 * A24: The rules the app applies to logs, audit records, notifications and the
 * diagnostic export.
 *
 * Read-only by design. The rules live in `redact_rules.json`, seeded from the
 * built-in set on first run, and that file — not `default_rules()` — is what
 * every redaction call reads. So letting the UI delete a rule would mean a
 * secret stops being redacted wherever the app writes: a security downgrade,
 * not a preference. The one mutation offered is restoring the built-in set,
 * which can only ever add rules back.
 *
 * Adding a rule is a plain text edit to `redact_rules.json` for now, which is
 * also how the sibling `copy_redact_rules.json` has always worked.
 */
export default function RedactionSettings() {
  const { t } = useI18n();
  const [rules, setRules] = useState<RedactRuleConfig[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);

  useEffect(() => {
    void api
      .listRedactRules()
      .then(setRules)
      .catch((err) => setMessage(String(err)));
  }, []);

  async function restoreDefaults() {
    setConfirming(false);
    setBusy(true);
    setMessage(null);
    try {
      setRules(await api.resetRedactRules());
    } catch (err) {
      setMessage(String(err));
      reportError("redaction-settings", "redaction rule reset failed", err);
    } finally {
      setBusy(false);
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
      <div className="grid max-h-36 gap-1.5 overflow-auto">
        {rules.length === 0 ? (
          <div className="text-xs text-muted-foreground">{t("No redaction rules.")}</div>
        ) : (
          rules.map((rule) => (
            <div
              key={rule.pattern}
              className="flex items-center gap-2 rounded-md border border-border bg-muted/35 px-2 py-1.5"
            >
              <code className="min-w-0 flex-1 truncate font-mono text-xs" title={rule.pattern}>
                {rule.pattern}
              </code>
              <span className="shrink-0 font-mono text-xs text-muted-foreground">
                {rule.replacement}
              </span>
            </div>
          ))
        )}
      </div>
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <span className="min-w-0 flex-1">
          {t("Rules live in redact_rules.json in your config folder.")}
        </span>
        <button
          type="button"
          className="shrink-0 rounded-md border border-input bg-card px-2 py-1 font-bold hover:bg-muted disabled:opacity-50"
          disabled={busy}
          onClick={() => setConfirming(true)}
        >
          {t("Restore defaults")}
        </button>
      </div>
      {message && <div className="break-words text-xs text-destructive">{message}</div>}
      {confirming && (
        <ConfirmDialog
          title={t("Restore the default redaction rules?")}
          description={t("This restores the built-in rules and discards any you added or removed.")}
          confirmLabel={t("Restore defaults")}
          confirmDisabled={busy}
          onConfirm={() => void restoreDefaults()}
          onCancel={() => setConfirming(false)}
        />
      )}
    </div>
  );
}
