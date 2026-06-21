// K3: in-app updater helpers. Wraps @tauri-apps/plugin-updater so the Settings
// panel can check for, download, and install a signed update. The plugin
// verifies the update's signature against the public key in tauri.conf.json
// before applying it, so a tampered update is rejected.

import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateCheckResult =
  | { status: "available"; version: string; notes?: string; date?: string }
  | { status: "up-to-date" }
  | { status: "error"; error: string };

/** Check the configured release endpoint for a newer signed build. */
export async function checkForUpdate(): Promise<UpdateCheckResult> {
  try {
    const update = await check();
    if (!update) return { status: "up-to-date" };
    return {
      status: "available",
      version: update.version,
      notes: update.body ?? undefined,
      date: update.date ?? undefined,
    };
  } catch (err) {
    return { status: "error", error: String(err) };
  }
}

/**
 * Download + install the pending update (signature is verified by the plugin),
 * then relaunch into the new version. `onProgress` reports bytes downloaded so
 * the UI can show progress.
 */
export async function downloadAndInstallUpdate(
  onProgress?: (downloaded: number, total: number | null) => void
): Promise<void> {
  const update = await check();
  if (!update) return;
  let downloaded = 0;
  let total: number | null = null;
  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case "Started":
        total = event.data.contentLength ?? null;
        break;
      case "Progress":
        downloaded += event.data.chunkLength;
        onProgress?.(downloaded, total);
        break;
      case "Finished":
        onProgress?.(downloaded, total);
        break;
    }
  });
  await relaunch();
}
