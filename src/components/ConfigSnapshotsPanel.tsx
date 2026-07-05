import { Camera, History, RefreshCw, RotateCcw, Trash2, Wand2 } from "lucide-react";
import { useEffect, useState } from "react";
import { api, reportError } from "../api";
import { CONFIG_TEMPLATES, type ConfigTemplate } from "../lib/configTemplates";
import { useI18n } from "../i18n";
import type { ConfigSnapshotInfo } from "../types";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { Dialog } from "./ui/dialog";
import { IconButton } from "./ui/icon-button";
import { Input } from "./ui/input";
import { EmptyState } from "./ui/state";
import { useToast } from "./ui/toast";

function formatTime(value?: string | null): string {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

/** V4-3: built-in policy/limits templates + snapshot save/restore of the config dir. */
export default function ConfigSnapshotsPanel() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [snapshots, setSnapshots] = useState<ConfigSnapshotInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [labelInput, setLabelInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [confirmTemplate, setConfirmTemplate] = useState<ConfigTemplate | null>(null);
  const [confirmRestore, setConfirmRestore] = useState<ConfigSnapshotInfo | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<ConfigSnapshotInfo | null>(null);

  async function refresh() {
    try {
      const list = await api.listConfigSnapshots();
      setSnapshots(list);
    } catch (err) {
      reportError("config-snapshots", "list snapshots failed", err);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function createSnapshot() {
    setBusy(true);
    try {
      await api.createConfigSnapshot(labelInput.trim());
      setLabelInput("");
      showToast("success", t("Snapshot saved"));
      await refresh();
    } catch (err) {
      showToast("error", String(err));
      reportError("config-snapshots", "create snapshot failed", err);
    } finally {
      setBusy(false);
    }
  }

  async function restoreSnapshot(snapshot: ConfigSnapshotInfo) {
    setBusy(true);
    try {
      await api.restoreConfigSnapshot(snapshot.id);
      showToast(
        "success",
        t("Restored. A backup of the previous state was saved automatically.")
      );
      await refresh();
    } catch (err) {
      showToast("error", String(err));
      reportError("config-snapshots", "restore snapshot failed", err, { id: snapshot.id });
    } finally {
      setBusy(false);
      setConfirmRestore(null);
    }
  }

  async function deleteSnapshot(snapshot: ConfigSnapshotInfo) {
    setBusy(true);
    try {
      await api.deleteConfigSnapshot(snapshot.id);
      await refresh();
    } catch (err) {
      showToast("error", String(err));
      reportError("config-snapshots", "delete snapshot failed", err, { id: snapshot.id });
    } finally {
      setBusy(false);
      setConfirmDelete(null);
    }
  }

  async function applyTemplate(template: ConfigTemplate) {
    setBusy(true);
    try {
      await api.applyConfigTemplate([
        ["policy.toml", template.policyToml],
        ["execution_limits.toml", template.limitsToml],
      ]);
      showToast(
        "success",
        t("Applied {name}. A snapshot of the previous config was saved automatically.", {
          name: template.name,
        })
      );
      await refresh();
    } catch (err) {
      showToast("error", String(err));
      reportError("config-snapshots", "apply template failed", err, { template: template.id });
    } finally {
      setBusy(false);
      setConfirmTemplate(null);
    }
  }

  return (
    <div className="grid gap-[18px]">
      <Card className="space-y-3 p-4">
        <div className="flex items-center gap-2 font-semibold">
          <Wand2 size={16} className="text-muted-foreground" />
          {t("Config templates")}
        </div>
        <p className="text-sm text-muted-foreground">
          {t(
            "Applying a template overwrites policy.toml and execution_limits.toml. Policy changes take effect immediately; limit changes need a daemon restart."
          )}
        </p>
        <div className="grid gap-3 md:grid-cols-3">
          {CONFIG_TEMPLATES.map((template) => (
            <div key={template.id} className="grid gap-2 rounded-lg border border-border bg-card p-3">
              <div className="font-semibold">{t(template.name)}</div>
              <p className="text-xs leading-relaxed text-muted-foreground">
                {t(template.description)}
              </p>
              <Button
                variant="secondary"
                size="sm"
                className="mt-1 justify-center"
                disabled={busy}
                onClick={() => setConfirmTemplate(template)}
              >
                {t("Apply template")}
              </Button>
            </div>
          ))}
        </div>
      </Card>

      <Card className="space-y-3 p-4">
        <div className="flex items-center gap-2 font-semibold">
          <History size={16} className="text-muted-foreground" />
          {t("Config snapshots")}
          <IconButton className="ml-auto" onClick={refresh} title={t("Refresh")}>
            <RefreshCw size={15} />
          </IconButton>
        </div>
        <div className="flex gap-2">
          <Input
            value={labelInput}
            onChange={(e) => setLabelInput(e.target.value)}
            placeholder={t("Snapshot label (optional)")}
          />
          <Button onClick={createSnapshot} disabled={busy} className="shrink-0">
            <Camera size={14} />
            {t("Save snapshot")}
          </Button>
        </div>

        {!loading && snapshots.length === 0 && (
          <EmptyState icon={History} title={t("No snapshots yet")} />
        )}

        {snapshots.length > 0 && (
          <div className="grid gap-2">
            {snapshots.map((snapshot) => (
              <div
                key={snapshot.id}
                className="flex flex-wrap items-center gap-3 rounded-lg border border-border bg-card p-3"
              >
                <div className="min-w-0 flex-1">
                  <div className="truncate font-semibold">
                    {snapshot.label ?? t("(unlabeled)")}
                  </div>
                  <div className="text-xs text-muted-foreground">
                    {formatTime(snapshot.created_at)} · {t("{count} files", { count: snapshot.files.length })}
                  </div>
                </div>
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={busy}
                  onClick={() => setConfirmRestore(snapshot)}
                >
                  <RotateCcw size={13} />
                  {t("Restore")}
                </Button>
                <IconButton
                  variant="danger"
                  disabled={busy}
                  title={t("Delete")}
                  onClick={() => setConfirmDelete(snapshot)}
                >
                  <Trash2 size={14} />
                </IconButton>
              </div>
            ))}
          </div>
        )}
      </Card>

      {confirmTemplate && (
        <Dialog onClose={() => setConfirmTemplate(null)} className="max-w-sm">
          <p className="mb-2">
            {t("Apply the {name} template?", { name: t(confirmTemplate.name) })}
          </p>
          <p className="rounded-md bg-warning/10 px-2.5 py-2 text-sm text-warning">
            {t("This overwrites policy.toml and execution_limits.toml. A snapshot is saved first.")}
          </p>
          <div className="mt-4 flex justify-end gap-2.5">
            <Button variant="secondary" onClick={() => setConfirmTemplate(null)}>
              {t("Cancel")}
            </Button>
            <Button onClick={() => applyTemplate(confirmTemplate)} disabled={busy}>
              {t("Apply template")}
            </Button>
          </div>
        </Dialog>
      )}

      {confirmRestore && (
        <Dialog onClose={() => setConfirmRestore(null)} className="max-w-sm">
          <p className="mb-2">
            {t("Restore snapshot {label}?", {
              label: confirmRestore.label ?? confirmRestore.id,
            })}
          </p>
          <p className="rounded-md bg-warning/10 px-2.5 py-2 text-sm text-warning">
            {t("This overwrites your current config with the snapshot's files. A backup of the current state is saved first.")}
          </p>
          <div className="mt-4 flex justify-end gap-2.5">
            <Button variant="secondary" onClick={() => setConfirmRestore(null)}>
              {t("Cancel")}
            </Button>
            <Button variant="destructive" onClick={() => restoreSnapshot(confirmRestore)} disabled={busy}>
              {t("Restore")}
            </Button>
          </div>
        </Dialog>
      )}

      {confirmDelete && (
        <Dialog onClose={() => setConfirmDelete(null)} className="max-w-sm">
          <p className="mb-2">
            {t("Delete snapshot {label}?", {
              label: confirmDelete.label ?? confirmDelete.id,
            })}
          </p>
          <div className="mt-4 flex justify-end gap-2.5">
            <Button variant="secondary" onClick={() => setConfirmDelete(null)}>
              {t("Cancel")}
            </Button>
            <Button variant="destructive" onClick={() => deleteSnapshot(confirmDelete)} disabled={busy}>
              {t("Delete")}
            </Button>
          </div>
        </Dialog>
      )}
    </div>
  );
}
