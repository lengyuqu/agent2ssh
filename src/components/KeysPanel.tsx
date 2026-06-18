import { Key, Plus, Trash2, Copy, Download, Check } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import type { SshKeyInfo } from "../types";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { IconButton } from "./ui/icon-button";
import { Input } from "./ui/input";
import { cn } from "../lib/utils";

type Props = Record<string, never>;

export default function KeysPanel(_props: Props) {
  const { t } = useI18n();
  const [keys, setKeys] = useState<SshKeyInfo[]>([]);
  const [showForm, setShowForm] = useState(false);
  const [newName, setNewName] = useState("");
  const [newComment, setNewComment] = useState("");
  const [importPath, setImportPath] = useState("");
  const [importName, setImportName] = useState("");
  const [mode, setMode] = useState<"generate" | "import">("generate");
  const [error, setError] = useState<string | null>(null);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  async function refresh() {
    try {
      setKeys(await api.listKeys());
    } catch {
      // keys dir might not exist yet
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function handleGenerate() {
    setError(null);
    if (!newName.trim()) {
      setError(t("Key name is required"));
      return;
    }
    try {
      await api.generateKey(newName.trim(), newComment.trim() || undefined);
      setNewName("");
      setNewComment("");
      setShowForm(false);
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleImport() {
    setError(null);
    if (!importPath.trim()) {
      setError(t("Source path is required"));
      return;
    }
    try {
      await api.importKey(importPath.trim(), importName.trim() || undefined);
      setImportPath("");
      setImportName("");
      setShowForm(false);
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleDelete(name: string) {
    if (!confirm(t('Delete key "{name}"?', { name }))) return;
    try {
      await api.deleteKey(name);
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  function copyPublicKey(pubKey: string, name: string) {
    navigator.clipboard.writeText(pubKey);
    setCopiedKey(name);
    setTimeout(() => setCopiedKey(null), 2000);
  }

  const tabCls = (active: boolean) =>
    cn(
      "flex flex-1 items-center justify-center gap-1 rounded-md border px-3 py-1.5 text-sm transition-colors",
      active
        ? "border-primary bg-primary text-primary-foreground"
        : "border-border bg-card text-foreground hover:bg-muted"
    );

  return (
    <Card className="space-y-3.5 p-4">
      <div className="flex items-center gap-2">
        <h3 className="flex items-center gap-2 font-semibold">
          <Key size={16} className="text-muted-foreground" />
          {t("SSH Keys")}
        </h3>
        <Button
          variant="secondary"
          size="sm"
          className="ml-auto"
          onClick={() => setShowForm(!showForm)}
        >
          <Plus size={14} />
          {showForm ? t("Cancel") : t("Add Key")}
        </Button>
      </div>

      {error && (
        <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {error}
        </div>
      )}

      {showForm && (
        <div className="grid gap-2 rounded-lg border border-border bg-muted/40 p-3">
          <div className="flex gap-1">
            <button className={tabCls(mode === "generate")} onClick={() => setMode("generate")}>
              {t("Generate New")}
            </button>
            <button className={tabCls(mode === "import")} onClick={() => setMode("import")}>
              <Download size={12} /> {t("Import")}
            </button>
          </div>

          {mode === "generate" ? (
            <>
              <Input
                placeholder={t("Key name (e.g. id-work)")}
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
              />
              <Input
                placeholder={t("Comment (optional)")}
                value={newComment}
                onChange={(e) => setNewComment(e.target.value)}
              />
              <Button onClick={handleGenerate}>{t("Generate Ed25519 Key")}</Button>
            </>
          ) : (
            <>
              <Input
                placeholder={t("Path to private key (e.g. ~/.ssh/id_rsa)")}
                value={importPath}
                onChange={(e) => setImportPath(e.target.value)}
              />
              <Input
                placeholder={t("Name (optional, defaults to filename)")}
                value={importName}
                onChange={(e) => setImportName(e.target.value)}
              />
              <Button onClick={handleImport}>{t("Import Key")}</Button>
            </>
          )}
        </div>
      )}

      {keys.length === 0 && !showForm && (
        <p className="px-1 py-2 text-sm text-muted-foreground">
          {t('No SSH keys managed. Click "Add Key" to get started.')}
        </p>
      )}

      {keys.length > 0 && (
        <table className="w-full border-collapse text-sm">
          <thead>
            <tr className="border-b border-border text-left text-muted-foreground">
              <th className="px-2 py-1.5 font-medium">{t("Name")}</th>
              <th className="px-2 py-1.5 font-medium">{t("Type")}</th>
              <th className="px-2 py-1.5 font-medium">{t("Public Key")}</th>
              <th className="px-2 py-1.5" />
            </tr>
          </thead>
          <tbody>
            {keys.map((k) => (
              <tr key={k.name} className="border-b border-border">
                <td className="px-2 py-1.5 font-mono">{k.name}</td>
                <td className="px-2 py-1.5">
                  <span className="rounded border border-border bg-muted px-1.5 py-0.5 text-[11px]">
                    {k.key_type}
                  </span>
                </td>
                <td className="px-2 py-1.5">
                  <div className="flex max-w-[300px] items-center gap-1">
                    {k.public_key ? (
                      <>
                        <code
                          className="inline-block max-w-[240px] truncate font-mono text-[11px]"
                          title={k.public_key}
                        >
                          {k.public_key.length > 50
                            ? k.public_key.slice(0, 50) + "..."
                            : k.public_key}
                        </code>
                        <IconButton
                          size="sm"
                          title={t("Copy public key")}
                          onClick={() => copyPublicKey(k.public_key, k.name)}
                        >
                          {copiedKey === k.name ? <Check size={12} /> : <Copy size={12} />}
                        </IconButton>
                      </>
                    ) : (
                      <span className="text-muted-foreground">{t("no public key")}</span>
                    )}
                  </div>
                </td>
                <td className="px-2 py-1.5">
                  <IconButton variant="danger" onClick={() => handleDelete(k.name)}>
                    <Trash2 size={14} />
                  </IconButton>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Card>
  );
}
