import { AlertCircle, File } from "lucide-react";
import Editor from "@monaco-editor/react";
import { useEffect, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import { useTheme } from "../theme";
import { ensureMonacoConfigured } from "../lib/monacoSetup";
import { languageForFile } from "../lib/previewLanguage";
import { Dialog } from "./ui/dialog";
import { LoadingState } from "./ui/state";

ensureMonacoConfigured();

// V3-1: files at or above this size skip the content fetch entirely and go
// straight to the metadata card — kept one byte under the backend's own
// SFTP_PREVIEW_MAX_BYTES/LOCAL_PREVIEW_MAX_BYTES cap so a file this panel
// considers previewable never gets rejected server-side.
const PREVIEW_MAX_BYTES = 1_000_000;

type Props = {
  onClose: () => void;
  name: string;
  path: string;
  size: number | null;
  mtime: number | null;
  kind: "remote" | "local";
  host: string;
};

function humanSize(bytes: number | null): string {
  if (bytes === null) return "";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

/** V3-1: inline preview for a selected SFTP-panel file — Monaco for text
 *  ≤1MB, a metadata card for everything else (binary, oversized, unreadable). */
export default function FilePreview({ onClose, name, path, size, mtime, kind, host }: Props) {
  const { t } = useI18n();
  const { theme } = useTheme();
  const [content, setContent] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);

  const tooLarge = size !== null && size >= PREVIEW_MAX_BYTES;

  useEffect(() => {
    if (tooLarge) return;
    let active = true;
    setLoading(true);
    setPreviewError(null);
    const read = kind === "remote" ? api.sftpReadText(host, path) : api.localReadText(path);
    read
      .then((result) => {
        if (!active) return;
        setContent(typeof result === "string" ? result : result.stdout);
      })
      .catch((err) => {
        if (!active) return;
        setPreviewError(String(err));
        reportError("file-preview", "read file for preview failed", err, { kind, path });
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kind, host, path, tooLarge]);

  const resolvedTheme =
    theme === "system"
      ? window.matchMedia("(prefers-color-scheme: light)").matches
        ? "light"
        : "dark"
      : theme;
  const monacoTheme = resolvedTheme === "light" || resolvedTheme === "solarized-light" ? "light" : "vs-dark";

  return (
    <Dialog onClose={onClose} className="max-w-4xl">
      <div className="mb-3 flex items-center gap-2 border-b border-border pb-3">
        <File size={16} className="shrink-0 text-muted-foreground" />
        <div className="min-w-0">
          <div className="truncate font-semibold">{name}</div>
          <div className="truncate text-xs text-muted-foreground" title={path}>
            {path}
          </div>
        </div>
      </div>

      {loading && <LoadingState label={t("Loading...")} />}

      {!loading && (tooLarge || previewError) && (
        <div className="grid gap-2 rounded-lg border border-border bg-muted/40 p-4">
          <div className="flex items-center gap-2 text-sm font-medium text-foreground">
            <AlertCircle size={15} className="text-muted-foreground" />
            {tooLarge ? t("File too large to preview") : t("Cannot preview this file")}
          </div>
          {!tooLarge && previewError && (
            <div className="text-xs text-muted-foreground">{previewError}</div>
          )}
          <dl className="mt-1 grid grid-cols-[80px_minmax(0,1fr)] gap-x-2 gap-y-1 text-xs">
            <dt className="text-muted-foreground">{t("Size")}</dt>
            <dd>{size !== null ? humanSize(size) : t("Unknown")}</dd>
            <dt className="text-muted-foreground">{t("Modified")}</dt>
            <dd>{mtime !== null ? new Date(mtime * 1000).toLocaleString() : t("Unknown")}</dd>
          </dl>
        </div>
      )}

      {!loading && !tooLarge && !previewError && content !== null && (
        <div className="h-[60vh] overflow-hidden rounded-lg border border-border">
          <Editor
            value={content}
            language={languageForFile(name)}
            theme={monacoTheme}
            options={{
              readOnly: true,
              minimap: { enabled: false },
              fontSize: 13,
              wordWrap: "on",
              scrollBeyondLastLine: false,
            }}
          />
        </div>
      )}
    </Dialog>
  );
}
