import {
  AlertTriangle,
  CheckCircle2,
  Cloud,
  Loader2,
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
import { useToast } from "./ui/toast";

type SyncAction = "load" | "save" | "test" | "push" | "refresh";

const defaultForm: WebDavSyncConfig = {
  enabled: false,
  url: "",
  username: "",
  remotePath: "agent2ssh/agent2ssh-sync.json",
  passwordConfigured: false,
};

function formatTime(value?: string | null): string {
  if (!value) return "Never";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function formatBytes(value?: number | null): string {
  if (!value) return "0 B";
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 / 1024).toFixed(2)} MB`;
}

export default function SyncPanel() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [form, setForm] = useState<WebDavSyncConfig>(defaultForm);
  const [password, setPassword] = useState("");
  const [status, setStatus] = useState<WebDavSyncStatus | null>(null);
  const [busy, setBusy] = useState<SyncAction | null>(null);

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
  const canUpload = form.enabled && canTest;

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
        remotePath: form.remotePath,
      });
      setForm(saved);
      setPassword("");
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

  async function testConnection() {
    setBusy("test");
    try {
      const nextStatus = await api.testWebDavSync();
      setStatus(nextStatus);
      showToast("success", nextStatus.lastMessage ?? t("WebDAV connection test completed."));
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
      showToast("success", nextStatus.lastMessage ?? t("WebDAV sync upload completed."));
    } catch (err) {
      showToast("error", String(err));
      reportError("sync-panel", "push webdav sync failed", err);
    } finally {
      setBusy(null);
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
                onChange={(event) => setPassword(event.target.value)}
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

          <div className="rounded-lg border border-warning/30 bg-warning/10 p-3 text-sm text-warning">
            <div className="flex items-start gap-2">
              <AlertTriangle size={16} className="mt-0.5 shrink-0" />
              <div className="grid gap-1 leading-snug">
                <p>
                  {t(
                    "Sync uploads a local configuration snapshot. Host records may include credentials if they are saved locally."
                  )}
                </p>
                <p>
                  {t(
                    "When enabled, sync runs every 10 minutes and after hosts, proxies, tunnels, or keys change."
                  )}
                </p>
              </div>
            </div>
          </div>

          <div className="flex flex-wrap justify-end gap-2">
            <Button type="submit" disabled={isBusy}>
              {busy === "save" ? <Loader2 size={14} className="animate-spin" /> : <Save size={14} />}
              {busy === "save" ? t("Saving...") : t("Save")}
            </Button>
            <Button
              variant="secondary"
              onClick={testConnection}
              disabled={isBusy || !canTest}
            >
              {busy === "test" ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                <RefreshCw size={14} />
              )}
              {busy === "test" ? t("Testing...") : t("Test connection")}
            </Button>
            <Button onClick={pushNow} disabled={isBusy || !canUpload}>
              {busy === "push" ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                <UploadCloud size={14} />
              )}
              {busy === "push" ? t("Uploading...") : t("Upload now")}
            </Button>
          </div>
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
              <Loader2 size={14} className="animate-spin" />
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
            <div className="mt-1 font-medium">{t(formatTime(status?.lastSyncAt))}</div>
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
    </div>
  );
}
