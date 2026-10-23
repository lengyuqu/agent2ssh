import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../api";
import { I18nProvider } from "../i18n";
import type { HostProfile, LocalDirListing, SftpDirEntry, SftpResult, WalkEntry } from "../types";
import SFTPPanel from "./SFTPPanel";
import { ToastProvider } from "./ui/toast";

vi.mock("../api", () => ({
  api: {
    setTrayLabels: vi.fn(),
    sftpReadDir: vi.fn(),
    localLs: vi.fn(),
    sftpDownload: vi.fn(),
    sftpUpload: vi.fn(),
    sftpCancel: vi.fn(),
    sftpRename: vi.fn(),
    sftpRemoveFile: vi.fn(),
    sftpWalk: vi.fn(),
    localMkdir: vi.fn(),
  },
  reportError: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("./ui/dialog", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./ui/dialog")>()),
  confirmDialog: vi.fn().mockResolvedValue(true),
}));

const host: HostProfile = {
  name: "server",
  host: "example.test",
  user: "tester",
  port: 22,
  group: "default",
};

function entry(name: string, isDir = false): SftpDirEntry {
  return { name, is_dir: isDir, size: isDir ? null : 12, modified_unix: 1700000000 };
}

function localListing(path: string, names: string[] = []): LocalDirListing {
  return {
    path,
    parent: path === "/" ? null : "/",
    home: "/home/tester",
    entries: names.map((name) => ({ name, is_dir: false, size: 12, modified_unix: 1700000000 })),
  };
}

const transferred = { bytes: 12 } as SftpResult;

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

function renderPanel() {
  return render(
    <I18nProvider>
      <ToastProvider>
        <SFTPPanel hosts={[host]} initialHost={host.name} />
      </ToastProvider>
    </I18nProvider>,
  );
}

async function openFolders(remotePath = "/tmp", localPath = "/downloads") {
  const remoteInput = screen.getByPlaceholderText("/home/user");
  fireEvent.change(remoteInput, { target: { value: remotePath } });
  fireEvent.keyDown(remoteInput, { key: "Enter" });
  const localInput = screen.getByPlaceholderText("~/Documents");
  fireEvent.change(localInput, { target: { value: localPath } });
  fireEvent.keyDown(localInput, { key: "Enter" });
  await waitFor(() => {
    expect(api.sftpReadDir).toHaveBeenCalledWith(host.name, remotePath);
    expect(api.localLs).toHaveBeenCalledWith(localPath);
    expect(screen.queryByText("Loading...")).toBeNull();
  });
}

function row(path: string) {
  // Testing Library normally normalizes edge whitespace, which is precisely
  // the filename identity this regression must preserve.
  return screen.getByTitle(path, { normalizer: (value) => value });
}

function selectRemoteFiles() {
  fireEvent.click(screen.getAllByRole("button", { name: "Select files" })[0]);
}

function startDownload() {
  fireEvent.click(screen.getByRole("button", { name: "To right" }));
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.setTrayLabels).mockResolvedValue(undefined);
  vi.mocked(api.sftpReadDir).mockResolvedValue([]);
  vi.mocked(api.localLs).mockResolvedValue(localListing("/downloads"));
  vi.mocked(api.sftpDownload).mockReset().mockResolvedValue(transferred);
  vi.mocked(api.sftpUpload).mockReset().mockResolvedValue(transferred);
  vi.mocked(api.sftpCancel).mockReset().mockResolvedValue(true);
  vi.mocked(api.sftpWalk).mockReset().mockResolvedValue([]);
  vi.mocked(api.localMkdir).mockReset().mockResolvedValue(undefined);
});

describe("SFTP filename and path identity", () => {
  it.each([
    "report  final.txt",
    "report -> old",
    " leading and trailing ",
    "tab\tname.txt",
    "line\nbreak.txt",
  ])("downloads the exact listed filename %j", async (name) => {
    vi.mocked(api.sftpReadDir).mockResolvedValue([entry(name)]);
    renderPanel();
    await openFolders();
    fireEvent.click(row(`/tmp/${name}`));
    startDownload();
    await waitFor(() => expect(api.sftpDownload).toHaveBeenCalledWith(
      host.name,
      `/tmp/${name}`,
      `/downloads/${name}`,
      { transferId: expect.any(String) },
    ));
    await screen.findByText(/Transferred 1 file\(s\)/);
  });

  it("renames the exact listed path and preserves the requested new filename", async () => {
    const name = " report  -> old ";
    vi.mocked(api.sftpReadDir).mockResolvedValue([entry(name)]);
    const prompt = vi.spyOn(window, "prompt").mockReturnValue(" renamed  -> new ");
    try {
      renderPanel();
      await openFolders();
      fireEvent.contextMenu(row(`/tmp/${name}`));
      fireEvent.click(screen.getByRole("menuitem", { name: "Rename" }));
      await waitFor(() => expect(api.sftpRename).toHaveBeenCalledWith(
        host.name, `/tmp/${name}`, "/tmp/ renamed  -> new ",
      ));
    } finally {
      prompt.mockRestore();
    }
  });

  it("deletes the exact filename including whitespace, an arrow, and a newline", async () => {
    const name = " report  -> old\n ";
    vi.mocked(api.sftpReadDir).mockResolvedValue([entry(name)]);
    renderPanel();
    await openFolders();
    fireEvent.contextMenu(row(`/tmp/${name}`));
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    await waitFor(() => expect(api.sftpRemoveFile).toHaveBeenCalledWith(host.name, `/tmp/${name}`));
  });

  it("keeps remote root navigation absolute and preserves directory edge spaces", async () => {
    vi.mocked(api.sftpReadDir).mockResolvedValue([entry(" folder ", true)]);
    renderPanel();
    await openFolders("/");
    fireEvent.click(row("/ folder "));
    await waitFor(() => expect(api.sftpReadDir).toHaveBeenLastCalledWith(host.name, "/ folder "));
    fireEvent.click(await screen.findByTitle("/ folder / folder ", { normalizer: (value) => value }));
    await waitFor(() => expect(api.sftpReadDir).toHaveBeenLastCalledWith(host.name, "/ folder / folder "));
  });

  it("downloads from remote root into POSIX local root using absolute paths", async () => {
    vi.mocked(api.sftpReadDir).mockResolvedValue([entry("report.txt")]);
    vi.mocked(api.localLs).mockResolvedValue(localListing("/"));
    renderPanel();
    await openFolders("/", "/");
    fireEvent.click(row("/report.txt"));
    startDownload();
    await waitFor(() => expect(api.sftpDownload).toHaveBeenCalledWith(
      host.name, "/report.txt", "/report.txt", { transferId: expect.any(String) },
    ));
    await screen.findByText(/Transferred 1 file\(s\)/);
  });

  it("uploads from POSIX local root into remote root using absolute paths", async () => {
    vi.mocked(api.localLs).mockResolvedValue(localListing("/", ["report.txt"]));
    renderPanel();
    await openFolders("/", "/");
    fireEvent.click(row("/report.txt"));
    fireEvent.click(screen.getByRole("button", { name: "To left" }));
    await waitFor(() => expect(api.sftpUpload).toHaveBeenCalledWith(
      host.name, "/report.txt", "/report.txt", { transferId: expect.any(String) },
    ));
    await screen.findByText(/Transferred 1 file\(s\)/);
  });

  it("continues joining Windows drive roots with Windows separators", async () => {
    vi.mocked(api.sftpReadDir).mockResolvedValue([entry("report.txt")]);
    vi.mocked(api.localLs).mockResolvedValue(localListing("C:\\"));
    renderPanel();
    await openFolders("/", "C:\\");
    fireEvent.click(row("/report.txt"));
    startDownload();
    await waitFor(() => expect(api.sftpDownload).toHaveBeenCalledWith(
      host.name, "/report.txt", "C:\\report.txt", { transferId: expect.any(String) },
    ));
    await screen.findByText(/Transferred 1 file\(s\)/);
  });
});

describe("SFTP transfer outcomes", () => {
  it("stops during directory discovery without creating directories or transferring files", async () => {
    const walk = deferred<WalkEntry[]>();
    vi.mocked(api.sftpReadDir).mockResolvedValue([entry("folder", true), entry("next", true)]);
    vi.mocked(api.sftpWalk).mockReturnValueOnce(walk.promise);
    renderPanel();
    await openFolders();
    fireEvent.click(within(row("/tmp/folder")).getByTitle("Select for transfer"));
    fireEvent.click(within(row("/tmp/next")).getByTitle("Select for transfer"));
    startDownload();
    await waitFor(() => expect(api.sftpWalk).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: /^Cancel$/ }));
    await act(async () => walk.resolve([{ rel_path: "report.txt", is_dir: false, size: 12 }]));
    await screen.findByText("Transfer cancelled after 0 file(s)");
    expect(api.sftpWalk).toHaveBeenCalledTimes(1);
    expect(api.localMkdir).not.toHaveBeenCalled();
    expect(api.sftpDownload).not.toHaveBeenCalled();
    expect(screen.queryByText(/^Transferred /)).toBeNull();
    expect(screen.getByText("2 selected")).toBeTruthy();
  });

  it("stops scheduling destination directories after cancellation during mkdir", async () => {
    const mkdir = deferred<void>();
    vi.mocked(api.sftpReadDir).mockResolvedValue([entry("folder", true)]);
    vi.mocked(api.sftpWalk).mockResolvedValue([
      { rel_path: "child", is_dir: true, size: 0 },
      { rel_path: "child/report.txt", is_dir: false, size: 12 },
    ]);
    vi.mocked(api.localMkdir).mockReturnValueOnce(mkdir.promise);
    renderPanel();
    await openFolders();
    fireEvent.click(within(row("/tmp/folder")).getByTitle("Select for transfer"));
    startDownload();
    await waitFor(() => expect(api.localMkdir).toHaveBeenCalledWith("/downloads/folder"));
    fireEvent.click(screen.getByRole("button", { name: /^Cancel$/ }));
    await act(async () => mkdir.resolve(undefined));
    await screen.findByText("Transfer cancelled after 0 file(s)");
    expect(api.localMkdir).toHaveBeenCalledTimes(1);
    expect(api.sftpDownload).not.toHaveBeenCalled();
    expect(screen.queryByText(/^Transferred /)).toBeNull();
    expect(screen.getByText("1 selected")).toBeTruthy();
  });

  it("does not report success or clear the selection when the first file is cancelled", async () => {
    const first = deferred<SftpResult>();
    vi.mocked(api.sftpReadDir).mockResolvedValue([entry("first"), entry("second")]);
    vi.mocked(api.sftpDownload).mockReturnValueOnce(first.promise);
    vi.mocked(api.sftpCancel).mockImplementation(async () => {
      first.reject(new Error("transfer cancelled"));
      return true;
    });
    renderPanel();
    await openFolders();
    selectRemoteFiles();
    startDownload();
    await waitFor(() => expect(api.sftpDownload).toHaveBeenCalledTimes(1));
    fireEvent.click(await screen.findByRole("button", { name: /^Cancel$/ }));
    await screen.findByText("Transfer cancelled after 0 file(s)");
    expect(api.sftpCancel).toHaveBeenCalledWith(vi.mocked(api.sftpDownload).mock.calls[0][3]?.transferId);
    expect(api.sftpDownload).toHaveBeenCalledTimes(1);
    expect(screen.queryByText(/^Transferred /)).toBeNull();
    expect(screen.getByText("2 selected")).toBeTruthy();
    expect(screen.getByRole<HTMLButtonElement>("button", { name: "To right" }).disabled).toBe(false);
  });

  it("reports only completed files after a partial cancellation and keeps the selection", async () => {
    const second = deferred<SftpResult>();
    vi.mocked(api.sftpReadDir).mockResolvedValue([entry("first"), entry("second"), entry("third")]);
    vi.mocked(api.sftpDownload).mockResolvedValueOnce(transferred).mockReturnValueOnce(second.promise);
    vi.mocked(api.sftpCancel).mockImplementation(async () => {
      second.reject(new Error("transfer cancelled"));
      return true;
    });
    renderPanel();
    await openFolders();
    selectRemoteFiles();
    startDownload();
    await waitFor(() => expect(api.sftpDownload).toHaveBeenCalledTimes(2));
    fireEvent.click(screen.getByRole("button", { name: /^Cancel$/ }));
    await screen.findByText("Transfer cancelled after 1 file(s)");
    expect(api.sftpDownload).toHaveBeenCalledTimes(2);
    expect(screen.queryByText(/^Transferred /)).toBeNull();
    expect(screen.getByText("3 selected")).toBeTruthy();
  });

  it("keeps a failed parallel batch active until all workers settle and starts no queued files", async () => {
    const workers = Array.from({ length: 4 }, () => deferred<SftpResult>());
    vi.mocked(api.sftpReadDir).mockResolvedValue(["one", "two", "three", "four", "five"].map((name) => entry(name)));
    for (const worker of workers) vi.mocked(api.sftpDownload).mockReturnValueOnce(worker.promise);
    renderPanel();
    await openFolders();
    selectRemoteFiles();
    fireEvent.click(screen.getByRole("checkbox", { name: "Parallel transfers" }));
    startDownload();
    await waitFor(() => expect(api.sftpDownload).toHaveBeenCalledTimes(4));
    await act(async () => workers[0].reject(new Error("disk full")));
    expect(screen.getByRole<HTMLButtonElement>("button", { name: "To right" }).disabled).toBe(true);
    expect(screen.queryByText("disk full")).toBeNull();
    await act(async () => {
      for (const worker of workers.slice(1)) worker.resolve(transferred);
    });
    await screen.findByText("Error: disk full");
    expect(api.sftpDownload).toHaveBeenCalledTimes(4);
    expect(screen.queryByText(/^Transferred /)).toBeNull();
    expect(screen.getByText("5 selected")).toBeTruthy();
    expect(screen.getByRole<HTMLButtonElement>("button", { name: "To right" }).disabled).toBe(false);
  });

  it("cancels every parallel worker and waits for late completions before finishing", async () => {
    const workers = Array.from({ length: 4 }, () => deferred<SftpResult>());
    vi.mocked(api.sftpReadDir).mockResolvedValue(["one", "two", "three", "four", "five"].map((name) => entry(name)));
    for (const worker of workers) vi.mocked(api.sftpDownload).mockReturnValueOnce(worker.promise);
    renderPanel();
    await openFolders();
    selectRemoteFiles();
    fireEvent.click(screen.getByRole("checkbox", { name: "Parallel transfers" }));
    startDownload();
    await waitFor(() => expect(api.sftpDownload).toHaveBeenCalledTimes(4));
    fireEvent.click(screen.getByRole("button", { name: /^Cancel$/ }));
    await act(async () => workers[0].reject(new Error("transfer cancelled")));
    expect(api.sftpCancel).toHaveBeenCalledTimes(4);
    expect(screen.getByRole<HTMLButtonElement>("button", { name: "To right" }).disabled).toBe(true);
    await act(async () => {
      workers[1].resolve(transferred);
      workers[2].reject(new Error("transfer cancelled"));
      workers[3].reject(new Error("transfer cancelled"));
    });
    await screen.findByText("Transfer cancelled after 1 file(s)");
    expect(api.sftpDownload).toHaveBeenCalledTimes(4);
    expect(screen.queryByText(/^Transferred /)).toBeNull();
    expect(screen.getByText("5 selected")).toBeTruthy();
  });

  it("reports a fully completed batch and clears its source selection", async () => {
    vi.mocked(api.sftpReadDir).mockResolvedValue([entry("first"), entry("second")]);
    renderPanel();
    await openFolders();
    selectRemoteFiles();
    startDownload();
    await screen.findByText(/Transferred 2 file\(s\)/);
    expect(api.sftpDownload).toHaveBeenCalledTimes(2);
    expect(screen.queryByText("2 selected")).toBeNull();
    expect(screen.getByRole<HTMLButtonElement>("button", { name: "To right" }).disabled).toBe(true);
  });

  it("does not offer cancellation after all files complete while refreshing the destination", async () => {
    const refreshed = deferred<LocalDirListing>();
    vi.mocked(api.sftpReadDir).mockResolvedValue([entry("first")]);
    vi.mocked(api.localLs).mockResolvedValueOnce(localListing("/downloads")).mockReturnValueOnce(refreshed.promise);
    renderPanel();
    await openFolders();
    selectRemoteFiles();
    startDownload();
    await waitFor(() => expect(api.localLs).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("button", { name: /^Cancel$/ })).toBeNull();
    await act(async () => refreshed.resolve(localListing("/downloads", ["first"])));
    await screen.findByText(/Transferred 1 file\(s\)/);
    expect(screen.queryByText(/^Transfer cancelled/)).toBeNull();
  });
});
