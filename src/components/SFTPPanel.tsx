import {
  ArrowDownUp,
  ArrowLeft,
  ArrowRight,
  CheckSquare,
  ChevronUp,
  File,
  Folder,
  FolderOpen,
  FolderPlus,
  HardDrive,
  Loader2,
  RefreshCw,
  Server,
  Square,
  X,
} from "lucide-react";
import { useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { HostProfile, LocalDirListing } from "../types";
import HostSelector from "./HostSelector";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { Dialog } from "./ui/dialog";
import { Input } from "./ui/input";

const labelCls = "grid gap-1.5 text-sm font-medium text-foreground/90";

type Side = "left" | "right";
type LocationKind = "remote" | "local";
type LsFilter = "all" | "folders" | "files";
type SortKey = "name" | "size" | "time";
type SortDir = "asc" | "desc";

type FileEntry = {
  name: string;
  isDir: boolean;
  size: number | null;
  /** Unix epoch seconds, when known. */
  mtime: number | null;
};

type SideState = {
  kind: LocationKind;
  host: string;
  path: string;
  entries: FileEntry[];
  selected: string[];
  loading: boolean;
  loaded: boolean;
  error: string | null;
  localParent: string | null;
  localHome: string | null;
  filter: LsFilter;
  sortKey: SortKey;
  sortDir: SortDir;
  /** Render cap (J3): a directory can have thousands of entries; only this many
   * rows are mounted at once, with a "show more" control to reveal the rest. */
  viewCap: number;
};

const VIEW_CAP_STEP = 400;
// K6: how many files transfer at once when parallel mode is enabled. Bounded so
// we don't open an unbounded number of SSH connections.
const PARALLEL_TRANSFERS = 4;

type TransferProgress = {
  total: number;
  done: number;
  current: string;
  bytesDone: number;
  bytesTotal: number;
  // K6: id of the file currently transferring, so a Cancel button can abort it.
  transferId?: string | null;
  cancelling?: boolean;
};

type PendingTransfer = {
  sourceSide: Side;
  destSide: Side;
  paths: string[];
  conflicts: string[];
};

type Props = {
  hosts: HostProfile[];
  initialHost?: string;
};

function makeSide(kind: LocationKind, host: string): SideState {
  return {
    kind,
    host,
    path: kind === "remote" ? "/tmp" : "",
    entries: [],
    selected: [],
    loading: false,
    loaded: false,
    error: null,
    localParent: null,
    localHome: null,
    filter: "all",
    sortKey: "name",
    sortDir: "asc",
    viewCap: VIEW_CAP_STEP,
  };
}

// ── Path helpers ─────────────────────────────────────────────────────────────

function trimPath(path: string): string {
  return path.trim();
}

function basenameOf(path: string): string {
  const normalized = trimPath(path).replace(/[\\/]+$/, "");
  const parts = normalized.split(/[\\/]/).filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1] : normalized;
}

function remoteParent(path: string): string {
  const normalized = trimPath(path).replace(/\/+$/, "");
  if (!normalized || normalized === ".") return ".";
  if (normalized === "/") return "/";
  const idx = normalized.lastIndexOf("/");
  return idx <= 0 ? "/" : normalized.slice(0, idx);
}

function remoteJoin(base: string, child: string): string {
  const normalized = trimPath(base).replace(/\/+$/, "");
  if (!normalized || normalized === ".") return child;
  if (normalized === "/") return `/${child}`;
  return `${normalized}/${child}`;
}

// K7: join a local base with a `/`-joined relative child, using the local
// platform's separator. A base that looks like a Windows path (contains a
// backslash or a drive letter) joins with `\` and converts the child's `/`
// separators to `\`, so reconstructed download paths aren't mixed-separator.
function localJoin(base: string, child: string): string {
  const normalized = trimPath(base).replace(/[\\/]+$/, "");
  if (!normalized) return child;
  const isWindows = /\\/.test(normalized) || /^[A-Za-z]:/.test(normalized);
  if (isWindows) {
    return `${normalized}\\${child.replace(/\//g, "\\")}`;
  }
  return `${normalized}/${child}`;
}

function joinForKind(kind: LocationKind, base: string, child: string): string {
  return kind === "remote" ? remoteJoin(base, child) : localJoin(base, child);
}

function buildBreadcrumbs(path: string): Array<{ label: string; path: string }> {
  const base = trimPath(path).replace(/\/+$/, "");
  if (!base || base === ".") return [{ label: "⌂", path: "/" }];
  const isAbsolute = base.startsWith("/");
  const parts = base.split("/").filter(Boolean);
  const out: Array<{ label: string; path: string }> = [{ label: "⌂", path: isAbsolute ? "/" : "." }];
  let current = isAbsolute ? "" : ".";
  for (const part of parts) {
    current = isAbsolute ? `${current}/${part}` : `${current}/${part}`;
    out.push({ label: part, path: current });
  }
  return out;
}

// ── Listing parse / format ───────────────────────────────────────────────────

const LS_MONTHS: Record<string, number> = {
  jan: 0,
  feb: 1,
  mar: 2,
  apr: 3,
  may: 4,
  jun: 5,
  jul: 6,
  aug: 7,
  sep: 8,
  oct: 9,
  nov: 10,
  dec: 11,
};

function parseLsMtime(month: string, day: string, timeOrYear: string): number | null {
  const monthIndex = LS_MONTHS[month.toLowerCase()];
  const dayOfMonth = Number.parseInt(day, 10);
  if (monthIndex === undefined || !Number.isFinite(dayOfMonth)) return null;
  const now = new Date();
  if (/^\d{4}$/.test(timeOrYear)) {
    return Math.floor(Date.UTC(Number(timeOrYear), monthIndex, dayOfMonth) / 1000);
  }
  const match = /^(\d{1,2}):(\d{2})$/.exec(timeOrYear);
  if (!match) return null;
  return Math.floor(
    Date.UTC(
      now.getUTCFullYear(),
      monthIndex,
      dayOfMonth,
      Number(match[1]),
      Number(match[2])
    ) / 1000
  );
}

function parseLsEntries(stdout: string): FileEntry[] {
  const parsed: FileEntry[] = [];
  for (const line of stdout.split("\n")) {
    const text = line.trim();
    if (!text) continue;
    if (/^total\s+\d+/i.test(text)) continue;

    const fields = text.split(/\s+/);
    if (fields.length >= 9) {
      const mode = fields[0];
      if (!mode) continue;
      const trailing = fields.slice(8).join(" ");
      const name = (trailing.split(" -> ")[0] ?? "").trim();
      if (!name || name === "." || name === "..") continue;

      const size = Number.parseInt(fields[4], 10);
      const parsedDate = parseLsMtime(fields[5], fields[6], fields[7]);
      parsed.push({
        name,
        isDir: mode.startsWith("d"),
        size: Number.isNaN(size) ? null : size,
        mtime: parsedDate,
      });
      continue;
    }

    if (!text.includes(" -> ") && !text.startsWith("d") && !text.startsWith("-") && text !== "..") {
      parsed.push({ name: text, isDir: false, size: null, mtime: null });
    }
  }
  return parsed;
}

function listingToEntries(listing: LocalDirListing): FileEntry[] {
  return listing.entries.map((entry) => ({
    name: entry.name,
    isDir: entry.is_dir,
    size: entry.is_dir ? null : entry.size,
    mtime: entry.modified_unix,
  }));
}

function humanSize(bytes: number | null): string {
  if (bytes === null) return "";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

function formatMtime(mtime: number | null): string {
  if (mtime === null) return "";
  const d = new Date(mtime * 1000);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleString(undefined, {
    year: "2-digit",
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function sortEntries(entries: FileEntry[], key: SortKey, dir: SortDir): FileEntry[] {
  const factor = dir === "asc" ? 1 : -1;
  return [...entries].sort((a, b) => {
    // Folders always group ahead of files regardless of sort key.
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    let cmp = 0;
    if (key === "size") cmp = (a.size ?? -1) - (b.size ?? -1);
    else if (key === "time") cmp = (a.mtime ?? -1) - (b.mtime ?? -1);
    else cmp = a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });
    if (cmp === 0) cmp = a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });
    return cmp * factor;
  });
}

function applyFilter(entries: FileEntry[], filter: LsFilter): FileEntry[] {
  if (filter === "folders") return entries.filter((e) => e.isDir);
  if (filter === "files") return entries.filter((e) => !e.isDir);
  return entries;
}

const DRAG_MIME = "application/x-agent2ssh-sftp";

export default function SFTPPanel({ hosts, initialHost = "" }: Props) {
  const { t } = useI18n();
  const [left, setLeft] = useState<SideState>(() => makeSide("remote", initialHost));
  const [right, setRight] = useState<SideState>(() => makeSide("local", ""));
  const [error, setError] = useState<string | null>(null);
  const [transfer, setTransfer] = useState<TransferProgress | null>(null);
  // K6: optional parallel transfers. Off by default (each transfer opens its own
  // SSH connection, so concurrency trades connection load for wall-clock time).
  const [parallel, setParallel] = useState(false);
  // K6: ids of all in-flight transfers, so Cancel aborts every running file —
  // not just one — when running in parallel.
  const activeTransferIds = useRef<Set<string>>(new Set());
  const transferCancelRequested = useRef(false);
  const [pending, setPending] = useState<PendingTransfer | null>(null);
  const [lastResult, setLastResult] = useState<{ count: number; dest: string; ms: number } | null>(null);

  const isTransferring = transfer !== null;

  function getSide(side: Side): SideState {
    return side === "left" ? left : right;
  }
  function setSide(side: Side, updater: (prev: SideState) => SideState) {
    if (side === "left") setLeft(updater);
    else setRight(updater);
  }
  function patchSide(side: Side, patch: Partial<SideState>) {
    setSide(side, (prev) => ({ ...prev, ...patch }));
  }

  function reportFailure(detail: string, err: unknown, ctx: Record<string, unknown>) {
    reportError("sftp-panel", detail, err, ctx);
  }

  // ── Loading directories ───────────────────────────────────────────────────

  async function loadRemote(side: Side, host: string, path: string) {
    if (!host) {
      patchSide(side, { error: t("Choose a host first") });
      return;
    }
    patchSide(side, { loading: true, error: null });
    try {
      const result = await api.sftpLs(host, path);
      patchSide(side, {
        entries: parseLsEntries(result.stdout),
        path,
        loading: false,
        loaded: true,
        selected: [],
        viewCap: VIEW_CAP_STEP,
        error: result.exit_code === 0 ? null : result.stderr || t("Could not read this folder"),
      });
    } catch (err) {
      patchSide(side, { loading: false, error: String(err) });
      reportFailure("remote ls failed", err, { side, host, path });
    }
  }

  async function loadLocal(side: Side, path: string | null) {
    patchSide(side, { loading: true, error: null });
    try {
      const listing = await api.localLs(path);
      patchSide(side, {
        entries: listingToEntries(listing),
        path: listing.path,
        localParent: listing.parent,
        localHome: listing.home,
        loading: false,
        loaded: true,
        selected: [],
        viewCap: VIEW_CAP_STEP,
        error: null,
      });
    } catch (err) {
      patchSide(side, { loading: false, error: String(err) });
      reportFailure("local ls failed", err, { side, path });
    }
  }

  function refreshSide(side: Side) {
    const s = getSide(side);
    if (s.kind === "remote") return loadRemote(side, s.host, s.path);
    return loadLocal(side, s.path || null);
  }

  function switchKind(side: Side, kind: LocationKind) {
    setSide(side, () => makeSide(kind, kind === "remote" ? initialHost : ""));
    if (kind === "local") void loadLocal(side, null);
  }

  function navigateTo(side: Side, path: string) {
    const s = getSide(side);
    if (s.kind === "remote") void loadRemote(side, s.host, path);
    else void loadLocal(side, path);
  }

  function toggleSelect(side: Side, full: string) {
    setSide(side, (prev) => ({
      ...prev,
      selected: prev.selected.includes(full)
        ? prev.selected.filter((p) => p !== full)
        : [...prev.selected, full],
    }));
  }

  function openEntry(side: Side, entry: FileEntry) {
    const s = getSide(side);
    // Row body: folders navigate, files toggle selection. Either can also be
    // selected for transfer via the leading checkbox (J4 covers folders).
    if (entry.isDir) {
      navigateTo(side, joinForKind(s.kind, s.path, entry.name));
      return;
    }
    toggleSelect(side, joinForKind(s.kind, s.path, entry.name));
  }

  function goUp(side: Side) {
    const s = getSide(side);
    if (s.kind === "remote") navigateTo(side, remoteParent(s.path));
    else if (s.localParent) navigateTo(side, s.localParent);
  }

  function selectAllFiles(side: Side) {
    const s = getSide(side);
    const files = applyFilter(s.entries, s.filter)
      .filter((e) => !e.isDir)
      .map((e) => joinForKind(s.kind, s.path, e.name));
    const allSelected = files.length > 0 && files.every((f) => s.selected.includes(f));
    patchSide(side, { selected: allSelected ? [] : files });
  }

  async function browseLocalFolder(side: Side) {
    try {
      const picked = await openDialog({ directory: true, multiple: false });
      if (typeof picked === "string") void loadLocal(side, picked);
    } catch (err) {
      reportFailure("local folder dialog failed", err, { side });
    }
  }

  async function addLocalFiles(side: Side) {
    try {
      const picked = await openDialog({ directory: false, multiple: true });
      const paths = Array.isArray(picked) ? picked : typeof picked === "string" ? [picked] : [];
      if (paths.length > 0) {
        setSide(side, (prev) => ({
          ...prev,
          selected: Array.from(new Set([...prev.selected, ...paths])),
        }));
      }
    } catch (err) {
      reportFailure("local file dialog failed", err, { side });
    }
  }

  async function makeDir(side: Side) {
    const s = getSide(side);
    const name = window.prompt(t("New folder name"));
    if (!name) return;
    const target = joinForKind(s.kind, s.path, name.trim());
    try {
      if (s.kind === "remote") {
        if (!s.host) return;
        await api.sftpMkdir(s.host, target);
      } else {
        await api.localMkdir(target);
      }
      await refreshSide(side);
    } catch (err) {
      setError(String(err));
      reportFailure("mkdir failed", err, { side, target });
    }
  }

  // ── Transfer ──────────────────────────────────────────────────────────────

  function transferableLabel(sourceSide: Side, destSide: Side): string | null {
    const s = getSide(sourceSide);
    const d = getSide(destSide);
    if (s.kind === "local" && d.kind === "local") return t("Local to local copy is not supported");
    return null;
  }

  function planTransfer(sourceSide: Side, destSide: Side): PendingTransfer | null {
    const s = getSide(sourceSide);
    const d = getSide(destSide);
    if (s.selected.length === 0) {
      setError(t("Select one or more files to transfer"));
      return null;
    }
    if (d.kind === "remote" && !d.host) {
      setError(t("Choose a destination host first"));
      return null;
    }
    const unsupported = transferableLabel(sourceSide, destSide);
    if (unsupported) {
      setError(unsupported);
      return null;
    }
    const destNames = new Set(d.entries.map((e) => e.name));
    const conflicts = s.selected.map(basenameOf).filter((name) => destNames.has(name));
    return { sourceSide, destSide, paths: [...s.selected], conflicts };
  }

  function startTransfer(sourceSide: Side, destSide: Side) {
    if (isTransferring) return;
    setError(null);
    setLastResult(null);
    const plan = planTransfer(sourceSide, destSide);
    if (!plan) return;
    if (plan.conflicts.length > 0) {
      setPending(plan);
      return;
    }
    void executeTransfer(plan);
  }

  // Expand the selection into concrete destination dirs to create and files to
  // transfer. Selected directories are walked recursively (J4); selected files
  // map straight through. `rel_path` from a walk is "/"-joined on both sides.
  async function buildTransferUnits(
    sourceSide: Side,
    destSide: Side,
    paths: string[]
  ): Promise<{ dirs: string[]; files: Array<{ src: string; dest: string; size: number }> }> {
    const s = getSide(sourceSide);
    const d = getSide(destSide);
    const entryByName = new Map(s.entries.map((e) => [e.name, e]));
    const dirs: string[] = [];
    const files: Array<{ src: string; dest: string; size: number }> = [];

    for (const path of paths) {
      const name = basenameOf(path);
      const entry = entryByName.get(name);
      if (!entry?.isDir) {
        files.push({ src: path, dest: joinForKind(d.kind, d.path, name), size: entry?.size ?? 0 });
        continue;
      }
      // Directory: enumerate its tree and recreate it under dest/<name>.
      const walk = s.kind === "local" ? await api.localWalk(path) : await api.sftpWalk(s.host, path);
      const destRoot = joinForKind(d.kind, d.path, name);
      dirs.push(destRoot);
      for (const w of walk) {
        const dest = joinForKind(d.kind, destRoot, w.rel_path);
        if (w.is_dir) dirs.push(dest);
        else files.push({ src: joinForKind(s.kind, path, w.rel_path), dest, size: w.size });
      }
    }
    return { dirs, files };
  }

  // K6: ask the backend to abort every in-flight transfer. The awaited calls in
  // the transfer workers then reject, surfacing through the catch in executeTransfer.
  async function cancelTransfer() {
    const ids = [...activeTransferIds.current];
    if (ids.length === 0) return;
    transferCancelRequested.current = true;
    setTransfer((prev) => (prev ? { ...prev, cancelling: true } : prev));
    try {
      await Promise.all(ids.map((id) => api.sftpCancel(id).catch(() => false)));
    } catch (err) {
      setError(String(err));
    }
  }

  // K6: transfer one file, registering its id so Cancel can abort it. Returns the
  // bytes moved so the caller can advance progress.
  async function transferOneFile(
    s: SideState,
    d: SideState,
    f: { src: string; dest: string; size: number }
  ): Promise<number> {
    if (transferCancelRequested.current) return 0;
    const transferId = `sftp-${Date.now()}-${Math.random().toString(36).slice(2)}`;
    activeTransferIds.current.add(transferId);
    setTransfer((prev) => (prev ? { ...prev, current: basenameOf(f.src), transferId } : prev));
    try {
      if (s.kind === "local" && d.kind === "remote") {
        return (await api.sftpUpload(d.host, f.src, f.dest, { transferId })).bytes;
      }
      if (s.kind === "remote" && d.kind === "local") {
        return (await api.sftpDownload(s.host, f.src, f.dest, { transferId })).bytes;
      }
      if (s.kind === "remote" && d.kind === "remote") {
        const result = await api.sftpExchange(s.host, f.src, d.host, f.dest);
        return result.uploaded.bytes || result.downloaded.bytes || f.size;
      }
      return 0;
    } finally {
      activeTransferIds.current.delete(transferId);
    }
  }

  async function executeTransfer(plan: PendingTransfer) {
    const s = getSide(plan.sourceSide);
    const d = getSide(plan.destSide);
    const started = performance.now();
    setError(null);
    transferCancelRequested.current = false;
    setTransfer({ total: 0, done: 0, current: t("Preparing…"), bytesDone: 0, bytesTotal: 0 });
    try {
      const { dirs, files } = await buildTransferUnits(plan.sourceSide, plan.destSide, plan.paths);
      const bytesTotal = files.reduce((sum, f) => sum + f.size, 0);
      setTransfer({ total: files.length, done: 0, current: "", bytesDone: 0, bytesTotal });

      // Create destination directories first (the walk lists parents before
      // children; mkdir is `-p`/create_dir_all so order and repeats are safe).
      for (const dir of dirs) {
        if (d.kind === "remote") await api.sftpMkdir(d.host, dir);
        else await api.localMkdir(dir);
      }

      // K6: transfer files through a bounded worker pool. Concurrency is 1 when
      // parallel mode is off (preserving the old serial behavior) and
      // PARALLEL_TRANSFERS otherwise. `aborted` makes the first failure (e.g. a
      // cancel) stop idle workers from pulling new files.
      activeTransferIds.current.clear();
      const concurrency = parallel ? Math.min(PARALLEL_TRANSFERS, files.length) : 1;
      let next = 0;
      let aborted = false;
      const worker = async () => {
        while (!aborted) {
          if (transferCancelRequested.current) return;
          const i = next++;
          if (i >= files.length) return;
          try {
            const bytes = await transferOneFile(s, d, files[i]);
            setTransfer((prev) =>
              prev ? { ...prev, done: prev.done + 1, bytesDone: prev.bytesDone + bytes } : prev
            );
          } catch (err) {
            if (transferCancelRequested.current) return;
            aborted = true;
            throw err;
          }
        }
      };
      await Promise.all(Array.from({ length: Math.max(1, concurrency) }, worker));
      setLastResult({
        count: files.length,
        dest: d.path,
        ms: Math.round(performance.now() - started),
      });
      // Clear the source selection and refresh the destination so the new files show.
      patchSide(plan.sourceSide, { selected: [] });
      await refreshSide(plan.destSide);
    } catch (err) {
      setError(String(err));
      reportFailure("transfer failed", err, {
        sourceKind: s.kind,
        destKind: d.kind,
        sourceHost: s.host,
        destHost: d.host,
      });
    } finally {
      transferCancelRequested.current = false;
      setTransfer(null);
    }
  }

  // ── Drag & drop ─────────────────────────────────────────────────────────────

  function onEntryDragStart(side: Side, entry: FileEntry, event: React.DragEvent) {
    const s = getSide(side);
    const full = joinForKind(s.kind, s.path, entry.name);
    // Drag the whole selection when the dragged file is part of it; otherwise just it.
    const paths = s.selected.includes(full) ? s.selected : [full];
    event.dataTransfer.setData(DRAG_MIME, JSON.stringify({ side, paths }));
    event.dataTransfer.effectAllowed = "copy";
  }

  function onPaneDrop(destSide: Side, event: React.DragEvent) {
    event.preventDefault();
    const raw = event.dataTransfer.getData(DRAG_MIME);
    if (!raw) return;
    try {
      const payload = JSON.parse(raw);
      if (
        typeof payload !== "object" ||
        payload === null ||
        (payload.side !== "left" && payload.side !== "right") ||
        !Array.isArray(payload.paths) ||
        !payload.paths.every((path: unknown) => typeof path === "string")
      ) {
        return;
      }
      const paths = payload.paths as string[];
      if (payload.side === destSide) return;
      if (isTransferring) return;
      const sourceSide = payload.side;
      // Reflect the dragged paths as the source selection, then run the normal plan.
      patchSide(sourceSide, { selected: paths });
      const d = getSide(destSide);
      const unsupported = transferableLabel(sourceSide, destSide);
      if (unsupported) {
        setError(unsupported);
        return;
      }
      if (d.kind === "remote" && !d.host) {
        setError(t("Choose a destination host first"));
        return;
      }
      const destNames = new Set(d.entries.map((e) => e.name));
      const conflicts = paths.map(basenameOf).filter((name) => destNames.has(name));
      const plan: PendingTransfer = { sourceSide, destSide, paths, conflicts };
      if (conflicts.length > 0) setPending(plan);
      else void executeTransfer(plan);
    } catch (err) {
      reportFailure("drop transfer failed", err, { destSide });
    }
  }

  // ── Rendering ───────────────────────────────────────────────────────────────

  function renderSortButton(side: Side, key: SortKey, label: string) {
    const s = getSide(side);
    const active = s.sortKey === key;
    return (
      <button
        type="button"
        onClick={() =>
          patchSide(side, {
            sortKey: key,
            sortDir: active && s.sortDir === "asc" ? "desc" : "asc",
          })
        }
        className={`inline-flex items-center gap-0.5 ${active ? "text-foreground" : "hover:text-foreground"}`}
      >
        {label}
        {active && <span className="text-[10px]">{s.sortDir === "asc" ? "▲" : "▼"}</span>}
      </button>
    );
  }

  function renderSidePanel(side: Side) {
    const s = getSide(side);
    const title = side === "left" ? t("Left") : t("Right");
    const visible = sortEntries(applyFilter(s.entries, s.filter), s.sortKey, s.sortDir);
    const fileCount = s.entries.filter((e) => !e.isDir).length;
    const canGoUp = s.kind === "remote" ? s.path !== "/" && s.path !== "." : !!s.localParent;

    return (
      <div
        className="flex min-h-[560px] flex-col gap-3 rounded-lg border border-border/70 bg-card/70 p-3"
        onDragOver={(e) => {
          if (e.dataTransfer.types.includes(DRAG_MIME)) {
            e.preventDefault();
            e.dataTransfer.dropEffect = "copy";
          }
        }}
        onDrop={(e) => onPaneDrop(side, e)}
      >
        {/* Header: location toggle */}
        <div className="flex items-center justify-between gap-2">
          <div className="text-sm font-semibold text-foreground/90">{title}</div>
          <div className="inline-flex overflow-hidden rounded-md border border-border">
            <button
              type="button"
              onClick={() => switchKind(side, "local")}
              className={`inline-flex items-center gap-1 px-2 py-1 text-xs ${
                s.kind === "local" ? "bg-primary text-primary-foreground" : "bg-card text-muted-foreground hover:text-foreground"
              }`}
            >
              <HardDrive size={12} />
              {t("This computer")}
            </button>
            <button
              type="button"
              onClick={() => switchKind(side, "remote")}
              className={`inline-flex items-center gap-1 px-2 py-1 text-xs ${
                s.kind === "remote" ? "bg-primary text-primary-foreground" : "bg-card text-muted-foreground hover:text-foreground"
              }`}
            >
              <Server size={12} />
              {t("Remote host")}
            </button>
          </div>
        </div>

        {s.kind === "remote" ? (
          <HostSelector
            hosts={hosts}
            value={s.host}
            onChange={(host) => {
              patchSide(side, { host, entries: [], loaded: false, selected: [] });
            }}
            label={t("Host")}
            disabled={isTransferring}
          />
        ) : (
          <div className="flex items-center gap-2">
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={() => browseLocalFolder(side)}
              disabled={isTransferring}
            >
              <FolderOpen size={14} />
              {t("Browse…")}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => addLocalFiles(side)}
              disabled={isTransferring}
            >
              <FolderPlus size={14} />
              {t("Add files…")}
            </Button>
            {s.localHome && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => loadLocal(side, s.localHome)}
                disabled={isTransferring}
                className="ml-auto"
              >
                {t("Home")}
              </Button>
            )}
          </div>
        )}

        {/* Path input + up */}
        <label className={labelCls}>
          {s.kind === "remote" ? t("Remote path") : t("Local path")}
          <div className="flex gap-2">
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={() => goUp(side)}
              disabled={isTransferring || !canGoUp}
              title={t("Parent folder")}
              className="shrink-0"
            >
              <ChevronUp size={14} />
            </Button>
            <Input
              value={s.path}
              onChange={(e) => patchSide(side, { path: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  navigateTo(side, s.path);
                }
              }}
              placeholder={s.kind === "remote" ? "/home/user" : "~/Documents"}
              disabled={isTransferring}
            />
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={() => refreshSide(side)}
              disabled={isTransferring || (s.kind === "remote" && !s.host)}
              className="shrink-0"
            >
              <RefreshCw size={14} className={s.loading ? "animate-spin" : ""} />
            </Button>
          </div>
        </label>

        {/* Filter + select all */}
        <div className="flex flex-wrap items-center gap-1.5 text-xs">
          {(["all", "folders", "files"] as LsFilter[]).map((f) => (
            <Button
              key={f}
              variant={s.filter === f ? "default" : "outline"}
              size="sm"
              onClick={() => patchSide(side, { filter: f })}
              className="h-7 px-2 text-xs"
            >
              {f === "all" ? t("All") : f === "folders" ? t("Folders") : t("Files")}
            </Button>
          ))}
          <Button
            variant="ghost"
            size="sm"
            onClick={() => selectAllFiles(side)}
            disabled={fileCount === 0}
            className="ml-auto h-7 px-2 text-xs"
          >
            {t("Select files")}
          </Button>
        </div>

        {/* Selection summary */}
        <div className="flex min-h-5 items-center justify-between gap-2 text-xs text-muted-foreground">
          <span>
            {s.selected.length > 0
              ? t("{count} selected", { count: s.selected.length })
              : t("{count} items", { count: visible.length })}
          </span>
          {s.selected.length > 0 && (
            <button
              type="button"
              onClick={() => patchSide(side, { selected: [] })}
              className="inline-flex items-center gap-1 hover:text-foreground"
            >
              <X size={12} />
              {t("Clear")}
            </button>
          )}
        </div>

        {/* Listing */}
        <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-border bg-background/70">
          <div className="flex flex-wrap gap-1 border-b border-border bg-card px-3 py-1.5">
            {buildBreadcrumbs(s.path).map((crumb, idx) => (
              <button
                key={`${crumb.path}-${idx}`}
                type="button"
                onClick={() => navigateTo(side, crumb.path)}
                className="rounded px-1.5 py-0.5 text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
                title={crumb.path}
              >
                {crumb.label}
              </button>
            ))}
          </div>

          {/* Column header */}
          <div className="flex items-center gap-2 border-b border-border bg-muted/40 px-3 py-1 text-[11px] text-muted-foreground">
            <span className="flex-1">{renderSortButton(side, "name", t("Name"))}</span>
            <span className="w-20 text-right">{renderSortButton(side, "size", t("Size"))}</span>
            <span className="w-32 text-right">{renderSortButton(side, "time", t("Modified"))}</span>
          </div>

          <div className="min-h-0 flex-1 overflow-auto p-1">
            {s.error ? (
              <div className="p-3 text-sm text-destructive">{s.error}</div>
            ) : s.loading ? (
              <div className="flex items-center justify-center gap-2 p-6 text-sm text-muted-foreground">
                <Loader2 size={16} className="animate-spin" />
                {t("Loading...")}
              </div>
            ) : !s.loaded ? (
              <div className="flex flex-col items-center justify-center gap-2 p-6 text-center text-sm text-muted-foreground">
                <FolderOpen size={26} />
                <div>
                  {s.kind === "remote" ? t("Choose a host and open a folder") : t("Open a local folder")}
                </div>
              </div>
            ) : visible.length === 0 ? (
              <div className="p-6 text-center text-sm text-muted-foreground">{t("(empty)")}</div>
            ) : (
              <>
                {visible.slice(0, s.viewCap).map((entry) => {
                const full = joinForKind(s.kind, s.path, entry.name);
                const selected = s.selected.includes(full);
                return (
                  <div
                    key={`${entry.name}-${entry.isDir}`}
                    draggable={!isTransferring}
                    onDragStart={(e) => onEntryDragStart(side, entry, e)}
                    onClick={() => openEntry(side, entry)}
                    title={full}
                    className={`group flex cursor-pointer items-center gap-2 rounded-md px-2.5 py-1.5 text-sm transition-colors hover:bg-muted ${
                      selected ? "bg-primary/12 text-primary" : ""
                    }`}
                  >
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        toggleSelect(side, full);
                      }}
                      title={t("Select for transfer")}
                      className="shrink-0 text-muted-foreground hover:text-foreground"
                    >
                      {selected ? (
                        <CheckSquare size={15} className="text-primary" />
                      ) : (
                        <Square size={15} />
                      )}
                    </button>
                    {entry.isDir ? (
                      <Folder size={15} className="shrink-0 text-warning" />
                    ) : (
                      <File size={15} className={`shrink-0 ${selected ? "text-primary" : "text-muted-foreground"}`} />
                    )}
                    <span className="flex-1 truncate">{entry.name}</span>
                    <span className="w-20 text-right text-xs text-muted-foreground">
                      {entry.isDir ? "" : humanSize(entry.size)}
                    </span>
                    <span className="w-32 truncate text-right text-xs text-muted-foreground">
                      {formatMtime(entry.mtime)}
                    </span>
                  </div>
                );
                })}
                {visible.length > s.viewCap && (
                  <button
                    type="button"
                    onClick={() => patchSide(side, { viewCap: s.viewCap + VIEW_CAP_STEP })}
                    className="mt-1 w-full rounded-md px-2 py-1.5 text-center text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
                  >
                    {t("Show more ({count} hidden)", { count: visible.length - s.viewCap })}
                  </button>
                )}
              </>
            )}
          </div>
        </div>

        {s.kind === "remote" && (
          <Button
            variant="secondary"
            size="sm"
            onClick={() => makeDir(side)}
            disabled={isTransferring || !s.host}
            className="self-start"
          >
            <FolderPlus size={14} />
            {t("New folder")}
          </Button>
        )}
      </div>
    );
  }

  function renderTransferButton(sourceSide: Side, destSide: Side, icon: React.ReactNode, label: string) {
    const s = getSide(sourceSide);
    const d = getSide(destSide);
    const unsupported = !!transferableLabel(sourceSide, destSide);
    const disabled = isTransferring || s.selected.length === 0 || unsupported || (d.kind === "remote" && !d.host);
    const verb =
      s.kind === "local" && d.kind === "remote"
        ? t("Upload")
        : s.kind === "remote" && d.kind === "local"
          ? t("Download")
          : t("Copy");
    return (
      <div className="grid gap-1">
        <Button
          size="sm"
          onClick={() => startTransfer(sourceSide, destSide)}
          disabled={disabled}
          className="w-full justify-center"
          title={unsupported ? t("Local to local copy is not supported") : undefined}
        >
          {icon}
          {label}
        </Button>
        <div className="rounded-md border border-dashed border-border px-2 py-1 text-center text-[11px] text-muted-foreground">
          {s.selected.length > 0 ? `${verb} · ${t("{count} selected", { count: s.selected.length })}` : t("Select source files")}
        </div>
      </div>
    );
  }

  return (
    <Card className="space-y-4 p-4">
      <div className="flex items-center gap-2 font-semibold">
        <ArrowDownUp size={16} className="text-muted-foreground" />
        {t("Files (SFTP)")}
      </div>

      {error && (
        <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2.5 text-sm text-destructive">
          {error}
        </div>
      )}

      {transfer && (
        <div className="rounded-lg border border-primary/30 bg-primary/10 px-3 py-2.5 text-sm text-primary">
          <div className="flex items-center gap-2">
            <Loader2 size={14} className="animate-spin" />
            {t("Transferring {done}/{total}", { done: transfer.done, total: transfer.total })}
            {transfer.bytesTotal > 0 && (
              <span className="text-primary/80">
                · {humanSize(transfer.bytesDone)} / {humanSize(transfer.bytesTotal)}
              </span>
            )}
            {transfer.current && <span className="truncate text-primary/80">· {transfer.current}</span>}
            {transfer.transferId && (
              <button
                type="button"
                className="ml-auto shrink-0 rounded border border-primary/40 px-2 py-0.5 text-xs font-medium text-primary hover:bg-primary/15 disabled:opacity-50"
                disabled={transfer.cancelling}
                onClick={() => void cancelTransfer()}
              >
                {transfer.cancelling ? t("Cancelling…") : t("Cancel")}
              </button>
            )}
          </div>
          <div className="mt-1.5 h-1.5 overflow-hidden rounded bg-primary/20">
            <div
              className="h-full rounded bg-primary transition-all"
              style={{
                width: `${
                  transfer.bytesTotal > 0
                    ? (transfer.bytesDone / transfer.bytesTotal) * 100
                    : transfer.total
                      ? (transfer.done / transfer.total) * 100
                      : 0
                }%`,
              }}
            />
          </div>
        </div>
      )}

      {lastResult && !transfer && (
        <div className="rounded-lg border border-success/30 bg-success/10 px-3 py-2.5 text-sm text-success">
          {t("Transferred {count} file(s) to {dest} in {ms}ms", {
            count: lastResult.count,
            dest: lastResult.dest,
            ms: lastResult.ms,
          })}
        </div>
      )}

      <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)]">
        {renderSidePanel("left")}

        <div className="flex items-center">
          <div className="grid w-full gap-3 rounded-lg border border-border/70 bg-muted/30 p-3 xl:w-44">
            {renderTransferButton("left", "right", <ArrowRight size={14} />, t("To right"))}
            {renderTransferButton("right", "left", <ArrowLeft size={14} />, t("To left"))}
            <label className="flex items-center gap-2 text-xs text-muted-foreground">
              <input
                type="checkbox"
                checked={parallel}
                onChange={(e) => setParallel(e.target.checked)}
                disabled={transfer !== null}
                className="size-3.5 shrink-0 accent-primary"
              />
              {t("Parallel transfers")}
            </label>
          </div>
        </div>

        {renderSidePanel("right")}
      </div>

      {pending && (
        <Dialog onClose={() => setPending(null)}>
          <div className="space-y-3">
            <div className="text-base font-semibold">{t("Overwrite existing files?")}</div>
            <div className="text-sm text-muted-foreground">
              {t("{count} file(s) already exist in the destination and will be replaced:", {
                count: pending.conflicts.length,
              })}
            </div>
            <div className="max-h-40 overflow-auto rounded-md border border-border bg-muted/40 p-2 text-xs font-mono">
              {pending.conflicts.map((name) => (
                <div key={name} className="truncate">
                  {name}
                </div>
              ))}
            </div>
            <div className="flex justify-end gap-2">
              <Button variant="outline" size="sm" onClick={() => setPending(null)}>
                {t("Cancel")}
              </Button>
              <Button
                size="sm"
                onClick={() => {
                  const plan = pending;
                  setPending(null);
                  void executeTransfer(plan);
                }}
              >
                {t("Overwrite")}
              </Button>
            </div>
          </div>
        </Dialog>
      )}
    </Card>
  );
}
