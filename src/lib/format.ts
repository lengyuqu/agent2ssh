/** Shared formatting helpers - date/time and byte sizes.
 *
 * Consolidates the per-component copies that previously lived in
 * ApprovalDetail, ApprovalQueue, ConfigSnapshotsPanel, LiveActivityPanel,
 * SyncPanel, ForwardPanel and RecordingsPanel. Behaviour is unchanged:
 * null/empty input falls back at the call site, invalid strings pass through.
 */

/** Locale date + time. `null`/empty -> "" ; an invalid string is returned as-is. */
export function formatDateTime(value?: string | number | Date | null): string {
  if (value === null || value === undefined || value === "") return "";
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) return typeof value === "string" ? value : "";
  return date.toLocaleString();
}

/** Locale clock time, optionally with Intl options. An invalid string is returned as-is. */
export function formatClockTime(
  value: string | number | Date,
  options?: Intl.DateTimeFormatOptions
): string {
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) return typeof value === "string" ? value : "";
  return date.toLocaleTimeString([], options);
}

/** Human byte size: B / KiB / MiB / GiB. `null`/`undefined` -> "0 B". */
export function formatBytes(value: number | null | undefined): string {
  if (value === null || value === undefined) return "0 B";
  if (value < 1024) return value + " B";
  if (value < 1024 * 1024) return (value / 1024).toFixed(1) + " KiB";
  if (value < 1024 * 1024 * 1024) return (value / (1024 * 1024)).toFixed(1) + " MiB";
  return (value / (1024 * 1024 * 1024)).toFixed(1) + " GiB";
}