import {
  ArrowDownToLine,
  ArrowUpFromLine,
  FolderOpen,
  FolderPlus,
  Info,
} from "lucide-react";
import { useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import type { ExecResult, SftpResult } from "../types";

type Props = {
  selectedHost: string;
};

export default function SFTPPanel({ selectedHost }: Props) {
  const { t } = useI18n();
  const [remotePath, setRemotePath] = useState("/tmp");
  const [localPath, setLocalPath] = useState("");
  const [lsResult, setLsResult] = useState<ExecResult | null>(null);
  const [transferResult, setTransferResult] = useState<SftpResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function listDir() {
    if (!selectedHost || !remotePath.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const result = await api.sftpLs(selectedHost, remotePath);
      setLsResult(result);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function makeDir() {
    if (!selectedHost || !remotePath.trim()) return;
    setBusy(true);
    setError(null);
    try {
      await api.sftpMkdir(selectedHost, remotePath);
      await listDir();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function statPath() {
    if (!selectedHost || !remotePath.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const result = await api.sftpStat(selectedHost, remotePath);
      setLsResult(result);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function upload() {
    if (!selectedHost || !localPath.trim() || !remotePath.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const result = await api.sftpUpload(selectedHost, localPath, remotePath);
      setTransferResult(result);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function download() {
    if (!selectedHost || !localPath.trim() || !remotePath.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const result = await api.sftpDownload(
        selectedHost,
        remotePath,
        localPath
      );
      setTransferResult(result);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel sftp-panel">
      <div className="panel-title">
        <FolderOpen size={16} />
        {t("Files (SFTP)")}
      </div>
      {error && <div className="error">{error}</div>}

      <label>
        {t("Remote path")}
        <input
          value={remotePath}
          onChange={(e) => setRemotePath(e.target.value)}
          placeholder="/home/user"
        />
      </label>
      <label>
        {t("Local path (for transfer)")}
        <input
          value={localPath}
          onChange={(e) => setLocalPath(e.target.value)}
          placeholder="~/Downloads/file.txt"
        />
      </label>

      <div className="sftp-actions">
        <button className="secondary" onClick={listDir} disabled={busy || !selectedHost}>
          <FolderOpen size={14} />
          {busy ? "..." : "ls"}
        </button>
        <button className="secondary" onClick={statPath} disabled={busy || !selectedHost}>
          <Info size={14} />
          stat
        </button>
        <button className="secondary" onClick={makeDir} disabled={busy || !selectedHost}>
          <FolderPlus size={14} />
          mkdir
        </button>
        <button className="primary" onClick={upload} disabled={busy || !selectedHost || !localPath}>
          <ArrowUpFromLine size={14} />
          {t("Upload")}
        </button>
        <button className="primary" onClick={download} disabled={busy || !selectedHost || !localPath}>
          <ArrowDownToLine size={14} />
          {t("Download")}
        </button>
      </div>

      {lsResult && (
        <div className="terminal-output">
          <div className="meta">
            exit={lsResult.exit_code ?? "signal"} {lsResult.duration_ms}ms
          </div>
          <pre>{lsResult.stdout || lsResult.stderr || t("(empty)")}</pre>
        </div>
      )}

      {transferResult && (
        <div className="transfer-result">
          {transferResult.direction === "upload" ? t("Uploaded") : t("Downloaded")}{" "}
          <code>{transferResult.local_path}</code> ↔{" "}
          <code>{transferResult.remote_path}</code> {t("in")}{" "}
          {transferResult.duration_ms}ms
        </div>
      )}
    </section>
  );
}
