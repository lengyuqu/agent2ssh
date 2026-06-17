import { Key, Plus, Trash2, Copy, Download, Check } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import type { SshKeyInfo } from "../types";

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
    if (!confirm(t("Delete key \"{name}\"?", { name }))) return;
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

  return (
    <div className="panel">
      <div className="panel-header">
        <h3><Key size={16} /> {t("SSH Keys")}</h3>
        <button className="secondary small" onClick={() => setShowForm(!showForm)}>
          <Plus size={14} />
          {showForm ? t("Cancel") : t("Add Key")}
        </button>
      </div>

      {error && <div className="error">{error}</div>}

      {showForm && (
        <div className="key-form">
          <div className="key-mode-toggle">
            <button
              className={mode === "generate" ? "active" : ""}
              onClick={() => setMode("generate")}
            >
              {t("Generate New")}
            </button>
            <button
              className={mode === "import" ? "active" : ""}
              onClick={() => setMode("import")}
            >
              <Download size={12} /> {t("Import")}
            </button>
          </div>

          {mode === "generate" ? (
            <>
              <input
                placeholder={t("Key name (e.g. id-work)")}
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
              />
              <input
                placeholder={t("Comment (optional)")}
                value={newComment}
                onChange={(e) => setNewComment(e.target.value)}
              />
              <button className="primary" onClick={handleGenerate}>
                {t("Generate Ed25519 Key")}
              </button>
            </>
          ) : (
            <>
              <input
                placeholder={t("Path to private key (e.g. ~/.ssh/id_rsa)")}
                value={importPath}
                onChange={(e) => setImportPath(e.target.value)}
              />
              <input
                placeholder={t("Name (optional, defaults to filename)")}
                value={importName}
                onChange={(e) => setImportName(e.target.value)}
              />
              <button className="primary" onClick={handleImport}>
                {t("Import Key")}
              </button>
            </>
          )}
        </div>
      )}

      {keys.length === 0 && !showForm && (
        <p className="empty">{t("No SSH keys managed. Click \"Add Key\" to get started.")}</p>
      )}

      {keys.length > 0 && (
        <table className="key-table">
          <thead>
            <tr>
              <th>{t("Name")}</th>
              <th>{t("Type")}</th>
              <th>{t("Public Key")}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {keys.map((k) => (
              <tr key={k.name}>
                <td className="mono">{k.name}</td>
                <td><span className="key-type-badge">{k.key_type}</span></td>
                <td className="pub-key-cell">
                  {k.public_key ? (
                    <>
                      <code title={k.public_key}>
                        {k.public_key.length > 50 ? k.public_key.slice(0, 50) + "..." : k.public_key}
                      </code>
                      <button
                        className="icon-button"
                        title={t("Copy public key")}
                        onClick={() => copyPublicKey(k.public_key, k.name)}
                      >
                        {copiedKey === k.name ? <Check size={12} /> : <Copy size={12} />}
                      </button>
                    </>
                  ) : (
                    <span className="empty">{t("no public key")}</span>
                  )}
                </td>
                <td>
                  <button className="icon-button danger" onClick={() => handleDelete(k.name)}>
                    <Trash2 size={14} />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
