import {
  BookMarked,
  Edit3,
  Loader2,
  Plus,
  RefreshCw,
  Save,
  Search,
  TerminalSquare,
  Trash2,
  X,
} from "lucide-react";
import { useCallback, useDeferredValue, useEffect, useMemo, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { Snippet } from "../types";
import { Button } from "./ui/button";
import { Dialog } from "./ui/dialog";
import { IconButton } from "./ui/icon-button";
import { Input } from "./ui/input";
import { Textarea } from "./ui/textarea";
import { useToast } from "./ui/toast";

type Props = {
  open: boolean;
  canInsert: boolean;
  onClose: () => void;
  onInsert: (command: string) => void;
};

type SnippetForm = {
  name: string;
  command: string;
  description: string;
};

const EMPTY_FORM: SnippetForm = { name: "", command: "", description: "" };

function toForm(snippet: Snippet): SnippetForm {
  return {
    name: snippet.name,
    command: snippet.command,
    description: snippet.description ?? "",
  };
}

/** Manage reusable commands and insert them into the active terminal input.
 * Insertion never appends Enter, so the user always reviews the command first. */
export default function SnippetsDialog({ open, canInsert, onClose, onInsert }: Props) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [editingName, setEditingName] = useState<string | null>(null);
  const [form, setForm] = useState<SnippetForm>(EMPTY_FORM);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setSnippets(await api.listSnippets());
    } catch (error) {
      showToast("error", t("Failed to load snippets: {error}", { error: String(error) }));
      reportError("snippets-dialog", "load snippets failed", error);
    } finally {
      setLoading(false);
    }
  }, [showToast, t]);

  useEffect(() => {
    if (open) void refresh();
  }, [open, refresh]);

  const filteredSnippets = useMemo(() => {
    const needle = deferredQuery.trim().toLocaleLowerCase();
    if (!needle) return snippets;
    return snippets.filter((snippet) =>
      `${snippet.name}\n${snippet.description ?? ""}\n${snippet.command}`
        .toLocaleLowerCase()
        .includes(needle)
    );
  }, [deferredQuery, snippets]);

  const startCreate = useCallback(() => {
    setEditingName("");
    setForm(EMPTY_FORM);
  }, []);

  const startEdit = useCallback((snippet: Snippet) => {
    setEditingName(snippet.name);
    setForm(toForm(snippet));
  }, []);

  const cancelEdit = useCallback(() => {
    setEditingName(null);
    setForm(EMPTY_FORM);
  }, []);

  async function handleSave() {
    const snippet: Snippet = {
      name: form.name.trim(),
      command: form.command.trim(),
      description: form.description.trim() || null,
    };
    if (!snippet.name || !snippet.command) {
      showToast("error", t("Snippet name and command are required"));
      return;
    }
    if (
      editingName !== snippet.name &&
      snippets.some((candidate) => candidate.name === snippet.name)
    ) {
      showToast("error", t("A snippet named {name} already exists", { name: snippet.name }));
      return;
    }

    setSaving(true);
    try {
      let next = await api.saveSnippet(snippet);
      if (editingName && editingName !== snippet.name) {
        await api.deleteSnippet(editingName);
        next = next.filter((candidate) => candidate.name !== editingName);
      }
      setSnippets(next);
      cancelEdit();
      showToast("success", t("Saved snippet: {name}", { name: snippet.name }));
    } catch (error) {
      showToast("error", t("Failed to save snippet: {error}", { error: String(error) }));
      reportError("snippets-dialog", "save snippet failed", error, { name: snippet.name });
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(snippet: Snippet) {
    if (!window.confirm(t("Delete snippet {name}?", { name: snippet.name }))) return;
    try {
      await api.deleteSnippet(snippet.name);
      setSnippets((current) => current.filter((candidate) => candidate.name !== snippet.name));
      if (editingName === snippet.name) cancelEdit();
      showToast("success", t("Deleted snippet: {name}", { name: snippet.name }));
    } catch (error) {
      showToast("error", t("Failed to delete snippet: {error}", { error: String(error) }));
      reportError("snippets-dialog", "delete snippet failed", error, { name: snippet.name });
    }
  }

  function handleInsert(snippet: Snippet) {
    if (!canInsert) return;
    onInsert(snippet.command);
  }

  if (!open) return null;

  return (
    <Dialog onClose={onClose} className="max-w-3xl p-0">
      <section role="dialog" aria-modal="true" aria-labelledby="snippets-title">
        <header className="flex items-center gap-2 border-b border-border px-4 py-3">
          <BookMarked size={17} className="text-primary" />
          <h2 id="snippets-title" className="font-semibold">
            {t("Command snippets")}
          </h2>
          <IconButton
            className="ml-auto"
            title={t("Refresh")}
            onClick={refresh}
            disabled={loading}
          >
            <RefreshCw size={14} className={loading ? "animate-spin" : ""} />
          </IconButton>
          <Button size="sm" variant="secondary" onClick={startCreate}>
            <Plus size={14} />
            {t("New")}
          </Button>
          <IconButton title={t("Close")} onClick={onClose}>
            <X size={14} />
          </IconButton>
        </header>

        <div className="grid gap-3 p-4">
          <label className="relative block">
            <Search
              size={14}
              className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground"
            />
            <Input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t("Search snippets")}
              aria-label={t("Search snippets")}
              className="pl-9"
            />
          </label>

          {editingName !== null && (
            <div className="grid gap-3 rounded-lg border border-border bg-muted/40 p-3">
              <label className="grid gap-1 text-xs font-medium text-muted-foreground">
                <span>{t("Name")}</span>
                <Input
                  value={form.name}
                  onChange={(event) =>
                    setForm((current) => ({ ...current, name: event.target.value }))
                  }
                  placeholder="check-disk"
                  autoFocus
                />
              </label>
              <label className="grid gap-1 text-xs font-medium text-muted-foreground">
                <span>{t("Description")}</span>
                <Input
                  value={form.description}
                  onChange={(event) =>
                    setForm((current) => ({ ...current, description: event.target.value }))
                  }
                  placeholder={t("What this command is for")}
                />
              </label>
              <label className="grid gap-1 text-xs font-medium text-muted-foreground">
                <span>{t("Command")}</span>
                <Textarea
                  value={form.command}
                  onChange={(event) =>
                    setForm((current) => ({ ...current, command: event.target.value }))
                  }
                  placeholder="df -h"
                  rows={4}
                />
              </label>
              <div className="flex gap-2">
                <Button size="sm" onClick={handleSave} disabled={saving}>
                  {saving ? <Loader2 size={14} className="animate-spin" /> : <Save size={14} />}
                  {t("Save")}
                </Button>
                <Button size="sm" variant="secondary" onClick={cancelEdit} disabled={saving}>
                  <X size={14} />
                  {t("Cancel")}
                </Button>
              </div>
            </div>
          )}

          <div className="max-h-[52vh] space-y-2 overflow-y-auto">
            {!loading && filteredSnippets.length === 0 && (
              <div className="rounded-lg border border-dashed border-border px-4 py-8 text-center text-sm text-muted-foreground">
                {query ? t("No matching snippets") : t("No snippets yet")}
              </div>
            )}
            {filteredSnippets.map((snippet) => (
              <article
                key={snippet.name}
                className="grid gap-2 rounded-lg border border-border bg-card p-3"
              >
                <div className="flex items-start gap-2">
                  <div className="min-w-0 flex-1">
                    <div className="font-semibold">{snippet.name}</div>
                    {snippet.description ? (
                      <div className="text-xs text-muted-foreground">{snippet.description}</div>
                    ) : null}
                  </div>
                  <Button
                    size="sm"
                    onClick={() => handleInsert(snippet)}
                    disabled={!canInsert}
                    title={
                      canInsert
                        ? t("Insert into focused terminal")
                        : t("Open and focus a terminal before inserting")
                    }
                  >
                    <TerminalSquare size={13} />
                    {t("Insert")}
                  </Button>
                  <IconButton title={t("Edit")} onClick={() => startEdit(snippet)}>
                    <Edit3 size={13} />
                  </IconButton>
                  <IconButton title={t("Delete")} onClick={() => void handleDelete(snippet)}>
                    <Trash2 size={13} />
                  </IconButton>
                </div>
                <pre className="m-0 overflow-x-auto whitespace-pre-wrap break-words rounded bg-muted/50 p-2 font-mono text-xs text-foreground/85">
                  {snippet.command}
                </pre>
              </article>
            ))}
          </div>
          {!canInsert && snippets.length > 0 ? (
            <p className="m-0 text-xs text-warning">
              {t("Open and focus a terminal before inserting a snippet.")}
            </p>
          ) : null}
        </div>
      </section>
    </Dialog>
  );
}
