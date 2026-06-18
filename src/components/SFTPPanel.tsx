import {
  ArrowLeftRight,
  File,
  Folder,
  FolderOpen,
  FolderPlus,
  Info,
} from "lucide-react";
import { useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { ExecResult, HostProfile, SftpExchangeResult } from "../types";
import HostSelector from "./HostSelector";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { Input } from "./ui/input";

const labelCls = "grid gap-1.5 text-sm font-medium text-foreground/90";

type Side = "left" | "right";
type BusyAction =
  | "idle"
  | "list-left"
  | "stat-left"
  | "mkdir-left"
  | "list-right"
  | "stat-right"
  | "mkdir-right"
  | "exchange-left-right"
  | "exchange-right-left";
type ExchangeDirection = "left-right" | "right-left";
type LsFilter = "all" | "folders" | "files";

type ParsedLsEntry = {
  name: string;
  isDirectory: boolean;
  raw: string;
  isParent?: boolean;
};

function looksLikeDirectoryPath(path: string): boolean {
  const trimmed = trimRemotePath(path);
  return !trimmed || trimmed === "/" || trimmed === "." || trimmed === ".." || /\/$/.test(trimmed);
}

function basenameOfPath(path: string): string | null {
  const normalized = trimRemotePath(path).replace(/\/+$/, "");
  if (!normalized || normalized === ".") return null;
  const parts = normalized.split("/").filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1] : null;
}

type Props = {
  hosts: HostProfile[];
  initialHost?: string;
};

function trimRemotePath(path: string): string {
  return path.trim();
}

function parentRemotePath(path: string): string {
  const normalized = trimRemotePath(path).replace(/\/+$/, "");
  if (!normalized || normalized === ".") return ".";
  if (normalized === "/") return "/";
  const idx = normalized.lastIndexOf("/");
  if (idx <= 0) return "/";
  return normalized.slice(0, idx);
}

function joinRemotePath(base: string, child: string): string {
  const normalized = trimRemotePath(base).replace(/\/+$/, "");
  if (!normalized || normalized === ".") return child;
  if (normalized === "/") return `/${child}`;
  return `${normalized}/${child}`;
}

function buildPathBreadcrumbs(path: string): Array<{ label: string; path: string }> {
  const normalized = trimRemotePath(path);
  const base = normalized.replace(/\/+$/, "");
  if (!base || base === ".") {
    return [{ label: "/", path: "/" }];
  }
  if (base === "/") {
    return [{ label: "/", path: "/" }];
  }
  const parts = base.split("/").filter(Boolean);
  const out: Array<{ label: string; path: string }> = [{ label: "/", path: "/" }];
  let current = "";
  for (const part of parts) {
    current += `/${part}`;
    out.push({ label: part, path: current });
  }
  return out;
}

function parseLsEntries(stdout: string): ParsedLsEntry[] {
  const parsed: ParsedLsEntry[] = [];

  for (const line of stdout.split("\n")) {
    const text = line.trim();
    if (!text) continue;
    if (/^total\s+\d+/i.test(text)) continue;

    const fields = text.split(/\s+/);
    if (fields.length >= 9) {
      const mode = fields[0];
      if (mode.length < 1) continue;

      const trailing = fields.slice(8).join(" ");
      const rawName = trailing.split(" -> ")[0] ?? "";
      const name = rawName.trim();
      if (!name || name === ".") continue;

      parsed.push({
        name,
        isDirectory: mode.startsWith("d"),
        raw: text,
        isParent: name === "..",
      });
      continue;
    }

    if (!text.includes(" -> ") && !text.startsWith("d") && !text.startsWith("-")) {
      parsed.push({ name: text, isDirectory: false, raw: text });
    }
  }

  return parsed.sort((a, b) => {
    if (a.isParent) return -1;
    if (b.isParent) return 1;
    if (a.isDirectory && !b.isDirectory) return -1;
    if (!a.isDirectory && b.isDirectory) return 1;
    return a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });
  });
}

function filterLsEntries(entries: ParsedLsEntry[], filter: LsFilter): ParsedLsEntry[] {
  if (filter === "all") return entries;
  if (filter === "folders") return entries.filter((entry) => entry.isDirectory);
  return entries.filter((entry) => !entry.isDirectory);
}

export default function SFTPPanel({ hosts, initialHost = "" }: Props) {
  const { t } = useI18n();
  const [leftHost, setLeftHost] = useState(initialHost);
  const [rightHost, setRightHost] = useState(initialHost);
  const [leftPath, setLeftPath] = useState("/tmp");
  const [rightPath, setRightPath] = useState("/tmp");
  const [leftSelectedFile, setLeftSelectedFile] = useState<string | null>(null);
  const [rightSelectedFile, setRightSelectedFile] = useState<string | null>(null);
  const [leftLsResult, setLeftLsResult] = useState<ExecResult | null>(null);
  const [rightLsResult, setRightLsResult] = useState<ExecResult | null>(null);
  const [leftEntries, setLeftEntries] = useState<ParsedLsEntry[]>([]);
  const [rightEntries, setRightEntries] = useState<ParsedLsEntry[]>([]);
  const [leftFilter, setLeftFilter] = useState<LsFilter>("all");
  const [rightFilter, setRightFilter] = useState<LsFilter>("all");
  const [exchangeResult, setExchangeResult] = useState<
    { direction: ExchangeDirection; result: SftpExchangeResult } | null
  >(null);
  const [busyAction, setBusyAction] = useState<BusyAction>("idle");
  const [error, setError] = useState<string | null>(null);

  const isBusy = busyAction !== "idle";

  async function withBusy(action: BusyAction, fn: () => Promise<unknown>, detail?: string) {
    if (isBusy) return;
    return (async () => {
      setBusyAction(action);
      setError(null);
      try {
        await fn();
      } catch (err) {
        setError(String(err));
        reportError("sftp-panel", detail ?? "sftp action failed", err, {
          action,
          sourceHost: leftHost,
          targetHost: rightHost,
        });
      } finally {
        setBusyAction("idle");
      }
    })();
  }

  function validateHostAndPath(side: Side, host: string, path: string): boolean {
    if (!host) {
      setError(side === "left" ? t("Source host is required") : t("Destination host is required"));
      return false;
    }
    if (!trimRemotePath(path)) {
      setError(side === "left" ? t("Source path is required") : t("Destination path is required"));
      return false;
    }
    return true;
  }

  function validateExchangeSide(sideLabel: "source" | "destination", host: string, path: string): boolean {
    if (!host) {
      setError(sideLabel === "source" ? t("Source host is required") : t("Destination host is required"));
      return false;
    }
    if (!trimRemotePath(path)) {
      setError(sideLabel === "source" ? t("Source path is required") : t("Destination path is required"));
      return false;
    }
    return true;
  }

  async function listDir(side: Side, explicitPath?: string) {
    const fromLeft = side === "left";
    const host = fromLeft ? leftHost : rightHost;
    const path = explicitPath ?? (fromLeft ? leftPath : rightPath);
    const action = fromLeft ? "list-left" : "list-right";
    if (!validateHostAndPath(side, host, path)) return;

    await withBusy(action, async () => {
      const result = await api.sftpLs(host, path);
      if (fromLeft) {
        setLeftLsResult(result);
        setLeftEntries(parseLsEntries(result.stdout));
        setLeftFilter("all");
        setLeftSelectedFile(null);
      } else {
        setRightLsResult(result);
        setRightEntries(parseLsEntries(result.stdout));
        setRightFilter("all");
        setRightSelectedFile(null);
      }
    }, "list path");
  }

  async function statPath(side: Side) {
    const fromLeft = side === "left";
    const host = fromLeft ? leftHost : rightHost;
    const path = fromLeft ? leftPath : rightPath;
    const action = fromLeft ? "stat-left" : "stat-right";
    if (!validateHostAndPath(side, host, path)) return;

    await withBusy(action, async () => {
      const result = await api.sftpStat(host, path);
      if (fromLeft) {
        setLeftLsResult(result);
      } else {
        setRightLsResult(result);
      }
    }, "stat path");
  }

  async function makeDir(side: Side) {
    const fromLeft = side === "left";
    const host = fromLeft ? leftHost : rightHost;
    const path = fromLeft ? leftPath : rightPath;
    const action = fromLeft ? "mkdir-left" : "mkdir-right";
    if (!validateHostAndPath(side, host, path)) return;

    await withBusy(action, async () => {
      const result = await api.sftpMkdir(host, path);
      if (fromLeft) {
        setLeftLsResult(result);
      } else {
        setRightLsResult(result);
      }
    }, "create directory");
  }

  function navigateToDirectory(side: Side, entry: ParsedLsEntry) {
    const fromLeft = side === "left";
    const source = trimRemotePath(fromLeft ? leftPath : rightPath);
    const nextPath = entry.isParent || entry.name === ".." ? parentRemotePath(source) : joinRemotePath(source, entry.name);

    if (fromLeft) {
      setLeftPath(nextPath);
      setLeftFilter("all");
      setLeftSelectedFile(null);
      void listDir("left", nextPath);
    } else {
      setRightPath(nextPath);
      setRightFilter("all");
      setRightSelectedFile(null);
      void listDir("right", nextPath);
    }
  }

  function jumpToPath(side: Side, path: string) {
    const target = trimRemotePath(path);
    if (side === "left") {
      setLeftPath(target);
      setLeftFilter("all");
      setLeftSelectedFile(null);
      void listDir("left", target);
    } else {
      setRightPath(target);
      setRightFilter("all");
      setRightSelectedFile(null);
      void listDir("right", target);
    }
  }

  function pickEntry(side: Side, entry: ParsedLsEntry) {
    const fromLeft = side === "left";
    const source = trimRemotePath(fromLeft ? leftPath : rightPath);

    if (fromLeft) {
      if (entry.isParent || entry.isDirectory) {
        const nextPath = entry.isParent || entry.name === ".." ? parentRemotePath(source) : joinRemotePath(source, entry.name);
        setLeftPath(nextPath);
        setLeftSelectedFile(null);
        return;
      }

      setLeftSelectedFile(joinRemotePath(source, entry.name));
    } else {
      if (entry.isParent || entry.isDirectory) {
        const nextPath = entry.isParent || entry.name === ".." ? parentRemotePath(source) : joinRemotePath(source, entry.name);
        setRightPath(nextPath);
        setRightSelectedFile(null);
        return;
      }

      setRightSelectedFile(joinRemotePath(source, entry.name));
    }
  }

  function resolveExchangeSourcePath(side: Side): string | null {
    const selected = side === "left" ? leftSelectedFile : rightSelectedFile;
    return selected ? selected : null;
  }

  function resolveExchangeDestinationPath(side: Side, sourcePath: string): string {
    const destinationBase = trimRemotePath(side === "left" ? leftPath : rightPath);
    if (!looksLikeDirectoryPath(destinationBase)) return destinationBase;

    const fileName = basenameOfPath(sourcePath);
    return fileName ? joinRemotePath(destinationBase, fileName) : destinationBase;
  }

  function canExchange(direction: ExchangeDirection): boolean {
    const sourceSide: Side = direction === "left-right" ? "left" : "right";
    const destinationSide: Side = direction === "left-right" ? "right" : "left";

    if (isBusy) return false;

    const sourceHost = sourceSide === "left" ? leftHost : rightHost;
    const destinationHost = destinationSide === "left" ? leftHost : rightHost;
    const sourcePath = resolveExchangeSourcePath(sourceSide);
    const destinationBase = trimRemotePath(destinationSide === "left" ? leftPath : rightPath);

    if (!sourceHost || !destinationHost) return false;
    if (!sourcePath || !destinationBase) return false;

    const resolvedDestinationPath = resolveExchangeDestinationPath(destinationSide, sourcePath);
    return !!resolvedDestinationPath;
  }

  async function exchange(direction: ExchangeDirection) {
    const sourceSide: Side = direction === "left-right" ? "left" : "right";
    const destinationSide: Side = direction === "left-right" ? "right" : "left";
    const sourceHost = sourceSide === "left" ? leftHost : rightHost;
    const destinationHost = destinationSide === "left" ? leftHost : rightHost;
    const sourcePath = resolveExchangeSourcePath(sourceSide);

    if (!sourcePath) {
      setError(t("Select a remote file to exchange"));
      return;
    }

    if (!validateExchangeSide("source", sourceHost, sourcePath)) return;
    if (
      !validateExchangeSide(
        "destination",
        destinationHost,
        trimRemotePath(destinationSide === "left" ? leftPath : rightPath)
      )
    )
      return;

    const resolvedDestinationPath = resolveExchangeDestinationPath(destinationSide, sourcePath);
    if (!resolvedDestinationPath) {
      setError(t("Destination path is required"));
      return;
    }

    const payload = {
      sourceHost,
      sourcePath,
      destinationHost,
      destinationPath: resolvedDestinationPath,
    };

    const action = direction === "left-right" ? "exchange-left-right" : "exchange-right-left";
    await withBusy(action, async () => {
      const result = await api.sftpExchange(
        payload.sourceHost,
        payload.sourcePath,
        payload.destinationHost,
        payload.destinationPath
      );
      setExchangeResult({ direction, result });
    }, "exchange files");
  }

  function renderSidePanel(side: Side) {
    const isLeft = side === "left";
    const host = isLeft ? leftHost : rightHost;
    const path = isLeft ? leftPath : rightPath;
    const result = isLeft ? leftLsResult : rightLsResult;
    const entries = isLeft ? leftEntries : rightEntries;
    const filter = isLeft ? leftFilter : rightFilter;
    const visibleEntries = filterLsEntries(entries, filter);
    const title = isLeft ? t("Source") : t("Destination");
    const listBusy = isLeft ? busyAction === "list-left" : busyAction === "list-right";
    const statBusy = isLeft ? busyAction === "stat-left" : busyAction === "stat-right";
    const mkdirBusy = isLeft ? busyAction === "mkdir-left" : busyAction === "mkdir-right";
    const actionBusy = isBusy || listBusy || statBusy || mkdirBusy;
    const normalizedPath = trimRemotePath(path);
    const hasDirectoryOutput = !!result && result.command.includes("ls ");
    const selectedFile = isLeft ? leftSelectedFile : rightSelectedFile;
    const clearSelection = isLeft
      ? () => setLeftSelectedFile(null)
      : () => setRightSelectedFile(null);

    return (
      <div className="space-y-2 rounded-lg border border-border/70 p-3">
        <div className="text-sm font-semibold text-foreground/90">{title}</div>
        <HostSelector
          hosts={hosts}
          value={host}
          onChange={isLeft ? setLeftHost : setRightHost}
          label={title}
          disabled={isBusy}
        />

        <label className={labelCls}>
          {t("Remote path")}
            <Input
            value={path}
            onChange={(event) =>
              isLeft
                ? (setLeftPath(event.target.value), setLeftSelectedFile(null))
                : (setRightPath(event.target.value), setRightSelectedFile(null))
            }
            placeholder="/home/user"
            disabled={isBusy}
          />
        </label>

        <div className="min-h-5 text-xs text-muted-foreground">
          {selectedFile ? (
            <div className="flex items-center justify-between gap-2">
              <span className="truncate">
                {t("Selected")}: <span className="text-foreground">{selectedFile}</span>
              </span>
              <Button
                variant="ghost"
                size="sm"
                onClick={clearSelection}
                className="h-auto px-2 py-0.5 text-xs"
              >
                {t("Clear")}
              </Button>
            </div>
          ) : (
            <span>{t("No file selected")}</span>
          )}
        </div>

      <div className="flex flex-wrap gap-2 [&>button]:min-w-20 [&>button]:flex-1">
          <Button
            variant="secondary"
            size="sm"
            onClick={() => listDir(side)}
            disabled={actionBusy || !host || !normalizedPath}
          >
            {listBusy ? "..." : "ls"}
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={() => statPath(side)}
            disabled={actionBusy || !host || !normalizedPath}
          >
            <Info size={14} />
            {statBusy ? "..." : "stat"}
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={() => makeDir(side)}
            disabled={actionBusy || !host || !normalizedPath}
          >
            <FolderPlus size={14} />
            {mkdirBusy ? "..." : "mkdir"}
          </Button>
        </div>

        <div className="mt-1 flex flex-wrap items-center gap-2 text-xs">
          <span className="text-muted-foreground">{t("Filter")}:</span>
          <Button
            variant={filter === "all" ? "default" : "outline"}
            size="sm"
            onClick={() => (isLeft ? setLeftFilter("all") : setRightFilter("all"))}
            disabled={actionBusy}
          >
            {t("All")}
          </Button>
          <Button
            variant={filter === "folders" ? "default" : "outline"}
            size="sm"
            onClick={() => (isLeft ? setLeftFilter("folders") : setRightFilter("folders"))}
            disabled={actionBusy}
          >
            {t("Folders")}
          </Button>
          <Button
            variant={filter === "files" ? "default" : "outline"}
            size="sm"
            onClick={() => (isLeft ? setLeftFilter("files") : setRightFilter("files"))}
            disabled={actionBusy}
          >
            {t("Only files")}
          </Button>
        </div>

        {result && (
          <div className="overflow-auto rounded-md bg-[#0e1620] text-[#e6edf3]">
            <div className="border-b border-white/10 px-3.5 py-2.5 text-[#8fb0c5]">
              {result.command}
              {" · "}
              {typeof result.exit_code === "number" ? `exit ${result.exit_code}` : t("N/A")}
            </div>

            <div className="flex flex-wrap gap-1 border-b border-white/10 px-3 py-1.5">
              {buildPathBreadcrumbs(normalizedPath).map((crumb) => (
                <button
                  key={crumb.path}
                  type="button"
                  onClick={() => jumpToPath(side, crumb.path)}
                  className="rounded px-1.5 py-0.5 text-xs text-[#8fb0c5] hover:bg-white/10"
                  title={crumb.path}
                >
                  {crumb.label === "/" ? "⌂" : crumb.label}
                </button>
              ))}
            </div>

            {hasDirectoryOutput && entries.length > 0 ? (
              <div className="space-y-1 p-2">
                {(normalizedPath !== "/" && normalizedPath !== "." ? [{ name: "..", isDirectory: true, raw: "../", isParent: true }, ...visibleEntries.filter((entry) => entry.name !== "..")] : visibleEntries).map((entry) => {
                  const isDir = entry.isDirectory;
                  const entryPath = entry.name === ".." ? parentRemotePath(normalizedPath) : joinRemotePath(normalizedPath, entry.name);
                  return (
                <button
                  type="button"
                  key={`${entry.name}-${entry.raw}`}
                  onClick={() => pickEntry(side, entry)}
                  onDoubleClick={() => {
                        if (isDir) {
                          navigateToDirectory(side, entry);
                        }
                  }}
                  className={`group flex w-full min-w-0 items-center gap-2 rounded px-2 py-1 text-left text-sm hover:bg-white/5 ${
                    entryPath === (isLeft ? leftSelectedFile : rightSelectedFile)
                      ? "bg-sky-600/25"
                      : ""
                  }`}
                  title={entryPath}
                >
                      {isDir ? <Folder size={14} className="shrink-0 text-amber-300" /> : <File size={14} className="shrink-0 text-sky-300" />}
                      <span className="truncate text-xs">{entry.name}</span>
                    </button>
                  );
                })}
              </div>
            ) : (
              <pre className="m-0 whitespace-pre-wrap break-words p-3.5 font-mono text-[13px]">
                {result.stdout || result.stderr || t("(empty)")}
              </pre>
            )}
          </div>
        )}
      </div>
    );
  }

  const canExchangeLeftRight = canExchange("left-right");
  const canExchangeRightLeft = canExchange("right-left");
  const leftToRightSource = resolveExchangeSourcePath("left");
  const leftToRightDestination = leftToRightSource
    ? resolveExchangeDestinationPath("right", leftToRightSource)
    : null;
  const rightToLeftSource = resolveExchangeSourcePath("right");
  const rightToLeftDestination = rightToLeftSource
    ? resolveExchangeDestinationPath("left", rightToLeftSource)
    : null;

  return (
    <Card className="space-y-4 p-4">
      <div className="flex items-center gap-2 font-semibold">
        <FolderOpen size={16} className="text-muted-foreground" />
        {t("Files (SFTP)")}
      </div>

      {error && (
        <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2.5 text-sm text-destructive">
          {error}
        </div>
      )}

      <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)]">
        {renderSidePanel("left")}

        <div className="flex items-center justify-center">
          <div className="flex flex-col gap-2">
            <Button
              size="sm"
              onClick={() => exchange("left-right")}
              disabled={!canExchangeLeftRight}
            >
              <ArrowLeftRight size={14} />
              {t("Exchange to right")}
            </Button>
            {leftToRightSource ? (
              <div className="max-w-40 truncate text-xs text-muted-foreground" title={leftToRightSource}>
                {leftToRightSource}
                <span className="text-foreground/80"> → </span>
                {leftToRightDestination ?? t("Destination path is required")}
              </div>
            ) : null}

            <Button
              size="sm"
              onClick={() => exchange("right-left")}
              disabled={!canExchangeRightLeft}
            >
              <ArrowLeftRight size={14} />
              {t("Exchange to left")}
            </Button>
            {rightToLeftSource ? (
              <div className="max-w-40 truncate text-xs text-muted-foreground" title={rightToLeftSource}>
                {rightToLeftSource}
                <span className="text-foreground/80"> → </span>
                {rightToLeftDestination ?? t("Destination path is required")}
              </div>
            ) : null}
          </div>
        </div>

        {renderSidePanel("right")}
      </div>

      {exchangeResult && (
        <div className="rounded-md bg-success/12 px-3.5 py-2.5 text-sm text-success">
          <div>
            {exchangeResult.direction === "left-right" ? t("Source") : t("Destination")} →{" "}
            {exchangeResult.direction === "left-right" ? t("Destination") : t("Source")}
          </div>
          <div className="mt-1 break-all">
            {exchangeResult.result.downloaded.remote_path}
            <span className="text-success/70"> → </span>
            {exchangeResult.result.uploaded.remote_path}
          </div>
          <div className="mt-1">
            {t("in")} {exchangeResult.result.duration_ms}ms
          </div>
        </div>
      )}
    </Card>
  );
}
