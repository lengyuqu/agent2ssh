import { useState } from "react";
import { Lock } from "lucide-react";
import { api } from "../api";
import { useI18n } from "../i18n";
import { Dialog } from "./ui/dialog";

type Props = {
  onUnlocked: () => void;
};

// K1: blocking startup dialog shown when the app-managed credential store is
// initialized but locked. The user must enter the master password to decrypt
// stored SSH passwords; until then, password-auth hosts are unavailable.
export default function SecretsUnlock({ onUnlocked }: Props) {
  const { t } = useI18n();
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!password || busy) return;
    setBusy(true);
    setError(null);
    try {
      await api.secretsUnlock(password);
      setPassword("");
      onUnlocked();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog>
      <form onSubmit={submit} className="space-y-4">
        <div className="flex items-center gap-2 text-base font-semibold">
          <Lock size={18} className="text-primary" />
          {t("Unlock credentials")}
        </div>
        <p className="text-sm text-muted-foreground">
          {t(
            "Enter your master password to decrypt saved SSH passwords. Stored credentials are encrypted on this machine; without the password, password-auth hosts are unavailable."
          )}
        </p>
        <input
          type="password"
          autoFocus
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          placeholder={t("Master password")}
          className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-ring"
        />
        {error && <div className="text-sm text-destructive">{error}</div>}
        <button
          type="submit"
          disabled={!password || busy}
          className="inline-flex h-9 w-full items-center justify-center rounded-md bg-primary text-sm font-bold text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-55"
        >
          {busy ? t("Unlocking…") : t("Unlock")}
        </button>
      </form>
    </Dialog>
  );
}
