import { Key, Plus, Trash2, Copy, Download, Check } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import type { SshKeyInfo } from "../types";

type Props = Record<string, never>;

export default function KeysPanel(_props: Props) {
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
      setError("Key name is required");
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
      setError("Source path is required");
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
    if (!confirm(`Delete key "${name}"?`)) return;
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
        <h3><Key size={16} /> SSH Keys</h3>
        <button className="secondary small" onClick={() => setShowForm(!showForm)}>
          <Plus size={14} />
          {showForm ? "Cancel" : "Add Key"}
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
              Generate New
            </button>
            <button
              className={mode === "import" ? "active" : ""}
              onClick={() => setMode("import")}
            >
              <Download size={12} /> Import
            </button>
          </div>

          {mode === "generate" ? (
            <>
              <input
                placeholder="Key name (e.g. id-work)"
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
              />
              <input
                placeholder="Comment (optional)"
                value={newComment}
                onChange={(e) => setNewComment(e.target.value)}
              />
              <button className="primary" onClick={handleGenerate}>
                Generate Ed25519 Key
              </button>
            </>
          ) : (
            <>
              <input
                placeholder="Path to private key (e.g. ~/.ssh/id_rsa)"
                value={importPath}
                onChange={(e) => setImportPath(e.target.value)}
              />
              <input
                placeholder="Name (optional, defaults to filename)"
                value={importName}
                onChange={(e) => setImportName(e.target.value)}
              />
              <button className="primary" onClick={handleImport}>
                Import Key
              </button>
            </>
          )}
        </div>
      )}

      {keys.length === 0 && !showForm && (
        <p className="empty">No SSH keys managed. Click "Add Key" to get started.</p>
      )}

      {keys.length > 0 && (
        <table className="key-table">
          <thead>
            <tr>
              <th>Name</th>
              <th>Type</th>
              <th>Public Key</th>
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
                        title="Copy public key"
                        onClick={() => copyPublicKey(k.public_key, k.name)}
                      >
                        {copiedKey === k.name ? <Check size={12} /> : <Copy size={12} />}
                      </button>
                    </>
                  ) : (
                    <span className="empty">no public key</span>
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
