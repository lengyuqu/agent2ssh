import { useEffect, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import { validateHighlightRule } from "../lib/terminal/highlight";
import type { HighlightRule } from "../types";

const emptyRule: HighlightRule = {
  name: "",
  keyword: "",
  color: "#FF6B6B",
  enabled: true,
  is_regex: false,
  is_case_sensitive: false,
};

function notifyChanged(): void {
  window.dispatchEvent(new Event("agent2ssh:highlights-changed"));
}

export default function HighlightSettings() {
  const { t } = useI18n();
  const [rules, setRules] = useState<HighlightRule[]>([]);
  const [draft, setDraft] = useState<HighlightRule>(emptyRule);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    void api
      .listHighlights()
      .then(setRules)
      .catch((err) => setMessage(String(err)));
  }, []);

  async function mutate(action: () => Promise<HighlightRule[]>) {
    setBusy(true);
    setMessage(null);
    try {
      setRules(await action());
      notifyChanged();
    } catch (err) {
      setMessage(String(err));
      reportError("highlight-settings", "highlight rule update failed", err);
    } finally {
      setBusy(false);
    }
  }

  function addRule() {
    const validation = validateHighlightRule(draft);
    if (validation) {
      setMessage(t(`Highlight validation: ${validation}`));
      return;
    }
    void mutate(async () => {
      const next = await api.addHighlight(draft);
      setDraft(emptyRule);
      return next;
    });
  }

  return (
    <div className="grid gap-2">
      <div className="grid max-h-36 gap-1.5 overflow-auto">
        {rules.map((rule) => (
          <div
            key={rule.keyword}
            className="flex items-center gap-2 rounded-md border border-border bg-muted/35 px-2 py-1.5"
          >
            <input
              type="checkbox"
              checked={rule.enabled}
              disabled={busy}
              aria-label={t("Enable {name}", { name: rule.name })}
              onChange={(event) =>
                void mutate(() =>
                  api.updateHighlight(rule.keyword, { ...rule, enabled: event.target.checked })
                )
              }
            />
            <span className="size-3 shrink-0 rounded-full" style={{ backgroundColor: rule.color }} />
            <span className="min-w-0 flex-1 truncate text-xs" title={rule.keyword}>
              {rule.name}
            </span>
            <button
              type="button"
              className="rounded px-1.5 py-0.5 text-xs text-destructive hover:bg-destructive/10"
              disabled={busy}
              onClick={() => void mutate(() => api.removeHighlight(rule.keyword))}
            >
              {t("Delete")}
            </button>
          </div>
        ))}
      </div>
      <div className="grid grid-cols-[1fr_1fr_auto] gap-1.5">
        <input
          value={draft.name}
          onChange={(event) => setDraft({ ...draft, name: event.target.value })}
          placeholder={t("Rule name")}
          className="h-8 min-w-0 rounded-md border border-input bg-background px-2 text-xs"
        />
        <input
          value={draft.keyword}
          onChange={(event) => setDraft({ ...draft, keyword: event.target.value })}
          placeholder={draft.is_regex ? t("Regular expression") : t("Keyword")}
          className="h-8 min-w-0 rounded-md border border-input bg-background px-2 font-mono text-xs"
        />
        <input
          type="color"
          value={draft.color}
          onChange={(event) => setDraft({ ...draft, color: event.target.value })}
          className="h-8 w-9 cursor-pointer rounded-md border border-input bg-background p-1"
          aria-label={t("Highlight color")}
        />
      </div>
      <div className="flex items-center gap-3 text-xs text-muted-foreground">
        <label className="flex items-center gap-1">
          <input
            type="checkbox"
            checked={draft.is_regex}
            onChange={(event) => setDraft({ ...draft, is_regex: event.target.checked })}
          />
          {t("Regex")}
        </label>
        <label className="flex items-center gap-1">
          <input
            type="checkbox"
            checked={draft.is_case_sensitive}
            onChange={(event) => setDraft({ ...draft, is_case_sensitive: event.target.checked })}
          />
          {t("Case sensitive")}
        </label>
        <button
          type="button"
          className="ml-auto rounded-md border border-input bg-card px-2 py-1 font-bold hover:bg-muted"
          disabled={busy}
          onClick={() => void mutate(() => api.resetHighlights())}
        >
          {t("Reset")}
        </button>
        <button
          type="button"
          className="rounded-md bg-primary px-2 py-1 font-bold text-primary-foreground disabled:opacity-50"
          disabled={busy || !draft.name.trim() || !draft.keyword.trim()}
          onClick={addRule}
        >
          {t("Add")}
        </button>
      </div>
      {message && <div className="break-words text-xs text-destructive">{message}</div>}
    </div>
  );
}
