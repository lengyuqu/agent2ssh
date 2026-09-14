import { useEffect, useState } from "react";
import { AlertTriangle, KeyRound, RotateCcw, Save } from "lucide-react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { AlgoPrefsState, SshAlgoPrefs } from "../types";
import { Button } from "./ui/button";
import { Dialog } from "./ui/dialog";
import { cn } from "../lib/utils";
import { InlineAlert, Spinner } from "./ui/state";

// A22: the eight `SshAlgoPrefs` fields, in the order they are negotiated. Each
// is a comma-delimited, most-preferred-first list handed to libssh2.
const FIELDS: Array<{ key: keyof SshAlgoPrefs; label: string }> = [
  { key: "kex", label: "Key exchange" },
  { key: "hostkey", label: "Host key" },
  { key: "cipher_cs", label: "Ciphers (client → server)" },
  { key: "cipher_sc", label: "Ciphers (server → client)" },
  { key: "mac_cs", label: "MACs (client → server)" },
  { key: "mac_sc", label: "MACs (server → client)" },
  { key: "comp_cs", label: "Compression (client → server)" },
  { key: "comp_sc", label: "Compression (server → client)" },
];

type Props = { onClose: () => void };

/**
 * A22: SSH algorithm preference editor.
 *
 * Opened from the Settings menu. Reads the effective preferences plus the
 * built-in defaults so each field can be flagged when it diverges, and offers a
 * one-click revert. Saving is validated on the Rust side (`ssh_algo::
 * save_algo_prefs`) because `apply_algo_prefs` is fail-closed — a single
 * unknown algorithm name breaks *every* connection, not just one.
 *
 * Changes apply to the next connection: `load_algo_prefs()` is read inside
 * `embedded_ssh` right before each handshake, so no restart is required, but
 * sessions that are already up keep the algorithms they negotiated.
 */
export default function AlgoPrefsDialog({ onClose }: Props) {
  const { t } = useI18n();
  const [state, setState] = useState<AlgoPrefsState | null>(null);
  const [draft, setDraft] = useState<SshAlgoPrefs | null>(null);
  const [busy, setBusy] = useState<"load" | "save" | "reset" | null>("load");
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void api
      .getAlgoPrefs()
      .then((next) => {
        if (cancelled) return;
        setState(next);
        setDraft(next.prefs);
      })
      .catch((err) => {
        if (cancelled) return;
        setError(String(err));
        reportError("algo-prefs", "load SSH algorithm preferences failed", err);
      })
      .finally(() => {
        if (!cancelled) setBusy(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // <Dialog> only closes on click-outside, which is thin for a form this tall —
  // let Escape dismiss it too.
  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  const dirty =
    state !== null && draft !== null && FIELDS.some((f) => draft[f.key] !== state.prefs[f.key]);

  async function save() {
    if (!draft || busy) return;
    setBusy("save");
    setMessage(null);
    setError(null);
    try {
      await api.setAlgoPrefs(draft);
      const next = await api.getAlgoPrefs();
      setState(next);
      setDraft(next.prefs);
      setMessage(t("Saved. New connections will use these algorithms."));
    } catch (err) {
      // The Rust validator returns a descriptive message naming the offending
      // field; surface it verbatim rather than a generic failure.
      setError(String(err));
      reportError("algo-prefs", "save SSH algorithm preferences failed", err);
    } finally {
      setBusy(null);
    }
  }

  async function reset() {
    if (busy) return;
    setBusy("reset");
    setMessage(null);
    setError(null);
    try {
      await api.clearAlgoPrefs();
      const next = await api.getAlgoPrefs();
      setState(next);
      setDraft(next.prefs);
      setMessage(t("Reverted to the built-in safe defaults."));
    } catch (err) {
      setError(String(err));
      reportError("algo-prefs", "reset SSH algorithm preferences failed", err);
    } finally {
      setBusy(null);
    }
  }

  return (
    <Dialog
      onClose={onClose}
      className="flex max-h-[calc(100vh-64px)] w-full max-w-2xl flex-col gap-3 overflow-hidden"
    >
      <div className="flex items-start justify-between gap-3 border-b border-border pb-2.5">
        <div className="grid min-w-0 gap-0.5">
          <strong className="flex items-center gap-1.5 text-[15px]">
            <KeyRound size={16} />
            {t("SSH algorithms")}
          </strong>
          <span className="text-xs leading-snug text-muted-foreground">
            {t(
              "Negotiation preferences passed to libssh2 before the handshake. Most-preferred first, comma-separated."
            )}
          </span>
        </div>
        <span
          className={cn(
            "shrink-0 rounded-full border px-2 py-0.5 text-[11px] font-bold",
            state?.custom
              ? "border-primary/40 bg-primary/10 text-primary"
              : "border-border bg-muted text-muted-foreground"
          )}
        >
          {state === null
            ? t("Loading...")
            : state.custom
              ? t("Custom")
              : t("Built-in defaults")}
        </span>
      </div>

      <div className="grid min-h-0 flex-1 gap-3 overflow-y-auto pr-0.5">
        {FIELDS.map((field) => {
          const value = draft?.[field.key] ?? "";
          const differs = state !== null && value !== state.defaults[field.key];
          return (
            <label key={field.key} className="grid gap-1">
              <span className="flex items-center gap-2 text-xs font-bold text-foreground/85">
                {t(field.label)}
                {differs && (
                  <span className="inline-flex items-center gap-1 rounded border border-warning/40 bg-warning/10 px-1.5 py-px text-[10px] font-bold text-warning">
                    <AlertTriangle size={10} />
                    {t("Differs from defaults")}
                  </span>
                )}
              </span>
              <textarea
                rows={2}
                spellCheck={false}
                aria-label={t(field.label)}
                value={value}
                disabled={busy !== null}
                onChange={(e) => {
                  if (!draft) return;
                  setDraft({ ...draft, [field.key]: e.target.value });
                }}
                className="w-full resize-y rounded-md border border-input bg-background px-2.5 py-1.5 font-mono text-xs leading-relaxed outline-none focus:ring-2 focus:ring-ring disabled:opacity-60"
              />
            </label>
          );
        })}

        <p className="rounded-md border border-border bg-muted/35 px-2.5 py-2 text-xs leading-snug text-muted-foreground">
          {t(
            "This build of libssh2 has no zlib support, so both compression fields must stay `none`. Any other value is rejected instead of silently breaking every connection."
          )}
        </p>

        {message && <div className="text-xs text-success">{message}</div>}
        {error && (
          <InlineAlert tone="destructive" bordered>
            {error}
          </InlineAlert>
        )}
      </div>

      <div className="flex flex-wrap items-center justify-between gap-2 border-t border-border pt-3">
        <span className="text-xs text-muted-foreground">
          {t("Applies to new connections; open sessions keep their negotiated algorithms.")}
        </span>
        <div className="flex items-center gap-2">
          <Button variant="outline" size="sm" onClick={reset} disabled={busy !== null}>
            {busy === "reset" ? <Spinner /> : <RotateCcw />}
            {t("Reset to defaults")}
          </Button>
          <Button
            variant="default"
            size="sm"
            onClick={save}
            disabled={busy !== null || !dirty}
            title={dirty ? t("Save algorithms") : t("No changes to save")}
          >
            <Save size={15} />
            {busy === "save" ? t("Saving...") : t("Save")}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
