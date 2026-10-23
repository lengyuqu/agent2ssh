import {
  AlertTriangle,
  CheckCircle2,
  Cloud,
  Download,
  RotateCcw,
  RefreshCw,
  Save,
  UploadCloud,
  XCircle,
} from "lucide-react";
import { FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { WebDavSyncConfig, WebDavSyncStatus } from "../types";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { Input } from "./ui/input";
import { InlineAlert, Spinner } from "./ui/state";
import { useToast } from "./ui/toast";
import { formatBytes, formatDateTime } from "../lib/format";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { ConfirmDialog } from "./ui/dialog";

type SyncAction = "load" | "save" | "test" | "push" | "refresh" | "download" | "restore";

const defaultForm: WebDavSyncConfig = {
  enabled: false,
  url: "",
  username: "",
  remotePath: "agent2ssh/agent2ssh-sync.json",
  passwordConfigured: false,
  syncPasswordConfigured: false,
};

export default function SyncPanel({ onRestored }: { onRestored?: () => Promise<void> } = {}) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [form, setForm] = useState<WebDavSyncConfig>(defaultForm);
  const [password, setPassword] = useState("");
  const [syncPassword, setSyncPassword] = useState("");
  const [status, setStatus] = useState<WebDavSyncStatus | null>(null);
  const [busy, setBusy] = useState<SyncAction | null>(null);
  const [confirmRestore, setConfirmRestore] = useState(false);
  const [recoveryMessage, setRecoveryMessage] = useState("");

  // T1-7: _setLocally tracks whether the form was edited locally and not
  // yet confirmed by the server. If the component re-mounts and fetches
  // stale data before the save completes, we discard the stale data to
  // prevent overwriting the user's unsaved edits.
  const formDirtyRef = useRef(false);
  const savingRef = useRef(false);

  // T1-6: Sync check request coalescing — multiple rapid refreshStatus()
  // calls (e.g. from useEffect + button + auto-sync) are merged into at most
  // one in-flight request. On network error, the previous status is retained
  // instead of being cleared, preventing UI flicker.
  const statusPromiseRef = useRef<Promise<WebDavSyncStatus> | null>(null);

  const isBusy = busy !== null;
  const canTest = useMemo(
    () => form.url.trim().length > 0 && form.remotePath.trim().length > 0,
    [form.remotePath, form.url]
  );
  const canUpload = form.enabled && canTest && form.syncPasswordConfigured;
  const canRecover = canTest && form.syncPasswordConfigured && !formDirtyRef.current;

  async function load() {
    setBusy("load");
    try {
      const [config, nextStatus] = await Promise.all([
        api.getWebDavSyncConfig(),
        api.getWebDavSyncStatus(),
      ]);
      // T1-7: Only apply server config if we're not in the middle of saving
      // or if the form hasn't been locally edited.
      if (!savingRef.current && !formDirtyRef.current) {
        setForm(config);
        setPassword("");
        setSyncPassword("");
      }
      setStatus(nextStatus);
    } catch (err) {
      showToast("error", String(err));
      reportError("sync-panel", "load webdav sync config failed", err);
    } finally {
      setBusy(null);
    }
  }

  useEffect(() => {
    load();
  }, []);

  // T1-6: Coalesced status refresh — N rapid calls collapse into one request.
  async function refreshStatus() {
    // If a refresh is already in flight, piggyback on it instead of firing a new request.
    if (statusPromiseRef.current) {
      return statusPromiseRef.current;
    }
    setBusy((prev) => prev ?? "refresh");
    const promise = api.getWebDavSyncStatus().finally(() => {
      statusPromiseRef.current = null;
    });
    statusPromiseRef.current = promise;
    try {
      const nextStatus = await promise;
      // T1-6: On success, update status.
      setStatus(nextStatus);
    } catch (err) {
      // T1-6: On network error, retain previous status — don't clear it.
      // Only show toast; the stale status is better than no status.
      showToast("error", String(err));
      reportError("sync-panel", "refresh webdav sync status failed", err);
    } finally {
      setBusy(null);
    }
  }

  async function save(event?: FormEvent) {
    event?.preventDefault();
    setBusy("save");
    savingRef.current = true;
    formDirtyRef.current = false;
    try {
      const saved = await api.setWebDavSyncConfig({
        enabled: form.enabled,
        url: form.url,
        username: form.username,
        password: password.trim().length > 0 ? password : null,
        syncPassword: syncPassword.length > 0 ? syncPassword : null,
        remotePath: form.remotePath,
      });
      setForm(saved);
      setPassword("");
      setSyncPassword("");
      setStatus(await api.getWebDavSyncStatus());
      showToast("success", t("WebDAV sync settings saved."));
    } catch (err) {
      // T1-7: On save failure, mark the form as dirty so a subsequent
      // load doesn't overwrite the user's edits.
      formDirtyRef.current = true;
      showToast("error", String(err));
      reportError("sync-panel", "save webdav sync config failed", err);
    } finally {
      savingRef.current = false;
      setBusy(null);
    }
  }

  // A failed WebDAV action still *resolves*: the backend reports failure through
  // `lastSuccess` rather than throwing, so the toast variant has to be derived
  // from the returned status. Reading only `catch` produced a green toast on a
  // failed upload.
  function showSyncOutcome(nextStatus: WebDavSyncStatus, action: "test" | "push") {
    if (nextStatus.lastSuccess === false) {
      const message =
        nextStatus.lastMessage ??
        t(
          action === "test"
            ? "WebDAV connection test failed."
            : "WebDAV sync upload failed."
        );
      showToast("error", message);
      reportError("sync-panel", `${action} webdav sync failed`, new Error(message));
      return;
    }
    showToast(
      "success",
      nextStatus.lastMessage ??
        t(
          action === "test"
            ? "WebDAV connection test completed."
            : "WebDAV sync upload completed."
        )
    );
  }

  async function testConnection() {
    setBusy("test");
    try {
      const nextStatus = await api.testWebDavSync();
      setStatus(nextStatus);
      showSyncOutcome(nextStatus, "test");
    } catch (err) {
      showToast("error", String(err));
      reportError("sync-panel", "test webdav sync failed", err);
    } finally {
      setBusy(null);
    }
  }

  async function pushNow() {
    setBusy("push");
    try {
      const nextStatus = await api.pushWebDavSync();
      setStatus(nextStatus);
      showSyncOutcome(nextStatus, "push");
    } catch (err) {
      showToast("error", String(err));
      reportError("sync-panel", "push webdav sync failed", err);
    } finally {
      setBusy(null);
    }
  }

  async function recover(restore: boolean) {
    setBusy(restore ? "restore" : "download");
    setRecoveryMessage("");
    try {
      let result;
      if (restore) {
        result = await api.restoreWebDavSync();
      } else {
        const outputPath = await saveDialog({ defaultPath: "agent2ssh-cloud-backup.a2s", filters: [{ name: "Agent2SSH encrypted backup", extensions: ["a2s"] }] });
        if (!outputPath) return;
        result = await api.downloadWebDavSync(outputPath);
      }
      const message = restore
        ? `${t("Cloud backup restored. Undo snapshot:")} ${result.backupId}`
        : `${t("Encrypted backup downloaded:")} ${result.localPath}`;
      setRecoveryMessage(message);
      showToast("success", message);
      // Recovery already succeeded: a refresh error must not imply rollback.
      try {
        setStatus(await api.getWebDavSyncStatus());
        if (restore) await onRestored?.();
      } catch (err) {
        showToast("error", t("Recovery completed, but refreshing the view failed. Reload the app."));
        reportError("sync-panel", "refresh after recovery failed", err);
      }
    } catch (err) {
      showToast("error", String(err));
      reportError("sync-panel", "cloud recovery failed", err);
      try { setStatus(await api.getWebDavSyncStatus()); } catch { /* Keep the original recovery error. */ }
    } finally {
      setBusy(null);
      setConfirmRestore(false);
    }
  }

  // T1-7: Mark form as dirty when user edits any field.
  function updateForm(updater: (current: WebDavSyncConfig) => WebDavSyncConfig) {
    formDirtyRef.current = true;
    setForm(updater);
  }

  const statusSuccess = status?.lastSuccess;

  return (
    <div className="grid gap-4 xl:grid-cols-[minmax(0,1.1fr)_minmax(320px,0.9fr)]">
      <Card className="p-4">
        <form className="space-y-4" onSubmit={save}>
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="flex items-center gap-2 font-semibold">
              <Cloud size={16} className="text-muted-foreground" />
              {t("WebDAV Sync")}
            </h3>
            <Badge variant={form.enabled ? "success" : "secondary"} className="ml-auto">
              {form.enabled ? t("Enabled") : t("Disabled")}
            </Badge>
          </div>

          <label className="flex items-center gap-2 rounded-lg border border-border bg-muted/35 px-3 py-2 text-sm">
            <input
              type="checkbox"
              className="size-4 accent-primary"
              checked={form.enabled}
              onChange={(event) =>
                updateForm((current) => ({ ...current, enabled: event.target.checked }))
              }
            />
            <span className="font-medium">{t("Enable WebDAV sync")}</span>
          </label>

          <div className="grid gap-3 md:grid-cols-2">
            <label className="space-y-1.5 md:col-span-2">
              <span className="text-xs font-semibold text-muted-foreground">
                {t("WebDAV URL")}
              </span>
              <Input
                value={form.url}
                onChange={(event) =>
                  updateForm((current) => ({ ...current, url: event.target.value }))
                }
                placeholder="https://example.com/dav"
                disabled={isBusy}
              />
            </label>

            <label className="space-y-1.5">
              <span className="text-xs font-semibold text-muted-foreground">
                {t("Username")}
              </span>
              <Input
                value={form.username ?? ""}
                onChange={(event) =>
                  updateForm((current) => ({ ...current, username: event.target.value }))
                }
                placeholder={t("Optional")}
                disabled={isBusy}
              />
            </label>

            <label className="space-y-1.5">
              <span className="text-xs font-semibold text-muted-foreground">
                {t("Password")}
              </span>
              <Input
                type="password"
                value={password}
                onChange={(event) => {
                  formDirtyRef.current = true;
                  setPassword(event.target.value);
                }}
                placeholder={
                  form.passwordConfigured
                    ? t("Saved. Leave blank to keep it.")
                    : t("Optional")
                }
                disabled={isBusy}
              />
            </label>

            <label className="space-y-1.5 md:col-span-2">
              <span className="text-xs font-semibold text-muted-foreground">
                {t("Sync encryption password")}
              </span>
              <Input
                type="password"
                value={syncPassword}
                onChange={(event) => {
                  formDirtyRef.current = true;
                  setSyncPassword(event.target.value);
                }}
                placeholder={
                  form.syncPasswordConfigured
                    ? t("Saved. Leave blank to keep it.")
                    : t("Required for uploads")
                }
                disabled={isBusy}
              />
              <p className="text-xs text-muted-foreground">
                {t("Stored in the credential vault. Set or unlock your master password first. Keep this sync password to decrypt your backups.")}
              </p>
            </label>

            <label className="space-y-1.5 md:col-span-2">
              <span className="text-xs font-semibold text-muted-foreground">
                {t("Remote file path")}
              </span>
              <Input
                value={form.remotePath}
                onChange={(event) =>
                  updateForm((current) => ({ ...current, remotePath: event.target.value }))
                }
                placeholder="agent2ssh/agent2ssh-sync.json"
                disabled={isBusy}
              />
            </label>
          </div>

          <InlineAlert bordered icon={AlertTriangle} className="rounded-lg p-3">
            <div className="grid gap-1 leading-snug">
              <p>
                {t(
                  "Sync uploads an encrypted configuration snapshot. Private SSH keys, credentials in the vault, and local trust records are excluded."
                )}
              </p>
              <p>
                {t(
                  "When enabled, sync runs every 10 minutes and after configuration changes."
                )}
              </p>
            </div>
          </InlineAlert>

          <div className="flex flex-wrap justify-end gap-2">
            <Button type="submit" disabled={isBusy}>
              {busy === "save" ? <Spinner size={14} /> : <Save size={14} />}
              {busy === "save" ? t("Saving...") : t("Save")}
            </Button>
            <Button
              variant="secondary"
              onClick={testConnection}
              disabled={isBusy || !canTest}
            >
              {busy === "test" ? (
                <Spinner size={14} />
              ) : (
                <RefreshCw size={14} />
              )}
              {busy === "test" ? t("Testing...") : t("Test connection")}
            </Button>
            <Button onClick={pushNow} disabled={isBusy || !canUpload}>
              {busy === "push" ? (
                <Spinner size={14} />
              ) : (
                <UploadCloud size={14} />
              )}
              {busy === "push" ? t("Uploading...") : t("Upload now")}
            </Button>
            <Button variant="secondary" onClick={() => recover(false)} disabled={isBusy || !canRecover}>
              {busy === "download" ? <Spinner size={14} /> : <Download size={14} />}
              {t("Download backup")}
            </Button>
            <Button variant="secondary" onClick={() => setConfirmRestore(true)} disabled={isBusy || !canRecover}>
              <RotateCcw size={14} />{t("Restore from cloud")}
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">{t("Save settings before downloading or restoring. Automatic upload can remain disabled.")}</p>
          {recoveryMessage && <p className="break-all text-xs" role="status">{recoveryMessage}</p>}
        </form>
      </Card>

      <Card className="p-4">
        <div className="flex items-center gap-2">
          <h3 className="font-semibold">{t("Sync Status")}</h3>
          <Button
            variant="secondary"
            size="sm"
            className="ml-auto"
            onClick={refreshStatus}
            disabled={isBusy}
          >
            {busy === "refresh" ? (
              <Spinner size={14} />
            ) : (
              <RefreshCw size={14} />
            )}
            {t("Refresh")}
          </Button>
        </div>

        <div className="mt-4 grid gap-2 text-sm">
          <div className="flex items-center justify-between gap-3 rounded-lg border border-border bg-muted/35 px-3 py-2">
            <span className="text-muted-foreground">{t("Configured")}</span>
            <Badge variant={status?.configured ? "success" : "secondary"}>
              {status?.configured ? t("Yes") : t("No")}
            </Badge>
          </div>
          <div className="rounded-lg border border-border bg-muted/35 px-3 py-2">
            <div className="flex items-center justify-between gap-3">
              <span className="text-muted-foreground">{t("Portable config state")}</span>
              <Badge
                variant={
                  status?.syncState === "in_sync"
                    ? "success"
                    : status?.syncState === "diverged"
                      ? "destructive"
                      : "secondary"
                }
              >
                {t((status?.syncState ?? "unknown").replace(/_/g, " "))}
              </Badge>
            </div>
            <div className="mt-2 text-xs text-muted-foreground">
              {t(status?.syncSummary ?? "Portable sync status is unavailable.")}
            </div>
            {status?.portableDigest ? (
              <div className="mt-2 break-all font-mono text-[11px]" title={status.portableDigest}>
                SHA-256 {status.portableDigest}
              </div>
            ) : null}
          </div>
          <div className="flex items-center justify-between gap-3 rounded-lg border border-border bg-muted/35 px-3 py-2">
            <span className="text-muted-foreground">{t("Last result")}</span>
            <span className="inline-flex items-center gap-1.5 font-medium">
              {statusSuccess === true ? (
                <>
                  <CheckCircle2 size={15} className="text-success" />
                  {t("Success")}
                </>
              ) : statusSuccess === false ? (
                <>
                  <XCircle size={15} className="text-destructive" />
                  {t("Failed")}
                </>
              ) : (
                t("No run yet")
              )}
            </span>
          </div>
          <div className="rounded-lg border border-border bg-muted/35 px-3 py-2">
            <span className="text-muted-foreground">{t("Last action")}</span>
            <div className="mt-1 font-medium">{status?.lastAction ?? t("None")}</div>
          </div>
          <div className="rounded-lg border border-border bg-muted/35 px-3 py-2">
            <span className="text-muted-foreground">{t("Last sync time")}</span>
            <div className="mt-1 font-medium">{t(status?.lastSyncAt ? formatDateTime(status.lastSyncAt) : "Never")}</div>
          </div>
          <div className="rounded-lg border border-border bg-muted/35 px-3 py-2">
            <span className="text-muted-foreground">{t("Uploaded size")}</span>
            <div className="mt-1 font-medium">{formatBytes(status?.lastUploadedBytes)}</div>
          </div>
          <div className="rounded-lg border border-border bg-muted/35 px-3 py-2">
            <span className="text-muted-foreground">{t("Remote path")}</span>
            <div className="mt-1 break-all font-mono text-xs">
              {status?.lastRemotePath ?? form.remotePath}
            </div>
          </div>
          <div className="rounded-lg border border-border bg-muted/35 px-3 py-2">
            <span className="text-muted-foreground">{t("Message")}</span>
            <div className="mt-1 break-words">{status?.lastMessage ?? t("No sync event recorded.")}</div>
          </div>
        </div>
      </Card>
      {confirmRestore && <ConfirmDialog
        title={t("Restore from cloud")}
        description={t("Replace portable configuration with the cloud backup? Files absent from the backup will be removed. A local undo snapshot is saved first. Credentials, private keys, and trust records are kept. Restart the app to apply restored preferences.")}
        confirmLabel={busy === "restore" ? t("Restoring...") : t("Restore")}
        confirmDisabled={isBusy}
        danger
        onConfirm={() => recover(true)}
        onCancel={() => { if (!isBusy) setConfirmRestore(false); }}
      />}
    </div>
  );
}
