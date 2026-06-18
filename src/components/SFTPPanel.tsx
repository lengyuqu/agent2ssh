import {
  ArrowDownToLine,
  ArrowUpFromLine,
  FolderOpen,
  FolderPlus,
  Info,
} from "lucide-react";
import { useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { ExecResult, HostProfile, SftpResult } from "../types";
import HostSelector from "./HostSelector";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { Input } from "./ui/input";

const labelCls = "grid gap-1.5 text-sm font-medium text-foreground/90";

type Props = {
  hosts: HostProfile[];
  initialHost?: string;
};

export default function SFTPPanel({ hosts, initialHost = "" }: Props) {
  const { t } = useI18n();
  const [targetHost, setTargetHost] = useState(initialHost);
  const [remotePath, setRemotePath] = useState("/tmp");
  const [localPath, setLocalPath] = useState("");
  const [lsResult, setLsResult] = useState<ExecResult | null>(null);
  const [transferResult, setTransferResult] = useState<SftpResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function listDir() {
    if (!targetHost || !remotePath.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const result = await api.sftpLs(targetHost, remotePath);
      setLsResult(result);
    } catch (err) {
      setError(String(err));
      reportError("sftp-panel", "sftp ls failed", err, { host: targetHost });
    } finally {
      setBusy(false);
    }
  }

  async function makeDir() {
    if (!targetHost || !remotePath.trim()) return;
    setBusy(true);
    setError(null);
    try {
      await api.sftpMkdir(targetHost, remotePath);
      await listDir();
    } catch (err) {
      setError(String(err));
      reportError("sftp-panel", "sftp mkdir failed", err, { host: targetHost });
    } finally {
      setBusy(false);
    }
  }

  async function statPath() {
    if (!targetHost || !remotePath.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const result = await api.sftpStat(targetHost, remotePath);
      setLsResult(result);
    } catch (err) {
      setError(String(err));
      reportError("sftp-panel", "sftp stat failed", err, { host: targetHost });
    } finally {
      setBusy(false);
    }
  }

  async function upload() {
    if (!targetHost || !localPath.trim() || !remotePath.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const result = await api.sftpUpload(targetHost, localPath, remotePath);
      setTransferResult(result);
    } catch (err) {
      setError(String(err));
      reportError("sftp-panel", "sftp upload failed", err, { host: targetHost });
    } finally {
      setBusy(false);
    }
  }

  async function download() {
    if (!targetHost || !localPath.trim() || !remotePath.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const result = await api.sftpDownload(targetHost, remotePath, localPath);
      setTransferResult(result);
    } catch (err) {
      setError(String(err));
      reportError("sftp-panel", "sftp download failed", err, { host: targetHost });
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card className="space-y-3 p-4">
      <div className="flex items-center gap-2 font-semibold">
        <FolderOpen size={16} className="text-muted-foreground" />
        {t("Files (SFTP)")}
      </div>
      {error && (
        <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2.5 text-sm text-destructive">
          {error}
        </div>
      )}
      <HostSelector hosts={hosts} value={targetHost} onChange={setTargetHost} disabled={busy} />

      <label className={labelCls}>
        {t("Remote path")}
        <Input
          value={remotePath}
          onChange={(e) => setRemotePath(e.target.value)}
          placeholder="/home/user"
        />
      </label>
      <label className={labelCls}>
        {t("Local path (for transfer)")}
        <Input
          value={localPath}
          onChange={(e) => setLocalPath(e.target.value)}
          placeholder="~/Downloads/file.txt"
        />
      </label>

      <div className="flex flex-wrap gap-2 [&>button]:min-w-20 [&>button]:flex-1">
        <Button variant="secondary" size="sm" onClick={listDir} disabled={busy || !targetHost}>
          <FolderOpen size={14} />
          {busy ? "..." : "ls"}
        </Button>
        <Button variant="secondary" size="sm" onClick={statPath} disabled={busy || !targetHost}>
          <Info size={14} />
          stat
        </Button>
        <Button variant="secondary" size="sm" onClick={makeDir} disabled={busy || !targetHost}>
          <FolderPlus size={14} />
          mkdir
        </Button>
        <Button size="sm" onClick={upload} disabled={busy || !targetHost || !localPath}>
          <ArrowUpFromLine size={14} />
          {t("Upload")}
        </Button>
        <Button size="sm" onClick={download} disabled={busy || !targetHost || !localPath}>
          <ArrowDownToLine size={14} />
          {t("Download")}
        </Button>
      </div>

      {lsResult && (
        <div className="overflow-auto rounded-md bg-[#0e1620] text-[#e6edf3]">
          <div className="border-b border-white/10 px-3.5 py-2.5 text-[#8fb0c5]">
            exit={lsResult.exit_code ?? "signal"} {lsResult.duration_ms}ms
          </div>
          <pre className="m-0 whitespace-pre-wrap break-words p-3.5 font-mono text-[13px]">
            {lsResult.stdout || lsResult.stderr || t("(empty)")}
          </pre>
        </div>
      )}

      {transferResult && (
        <div className="rounded-md bg-success/12 px-3.5 py-2.5 text-sm text-success">
          {transferResult.direction === "upload" ? t("Uploaded") : t("Downloaded")}{" "}
          <code className="rounded bg-black/10 px-1 py-px">{transferResult.local_path}</code> ↔{" "}
          <code className="rounded bg-black/10 px-1 py-px">{transferResult.remote_path}</code>{" "}
          {t("in")} {transferResult.duration_ms}ms
        </div>
      )}
    </Card>
  );
}
