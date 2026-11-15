/**
 * A tiny pub/sub carrying live SFTP transfer progress from the embedded file
 * panels up to the workspace-level transfer center. The panel keeps owning
 * its transfer state; the bus is a read-only mirror for aggregation, so a
 * closed drawer never hides what is still moving.
 */

export type TransferSnapshot = {
  total: number;
  done: number;
  current: string;
  bytesDone: number;
  bytesTotal: number;
  cancelling?: boolean;
  cancellable: boolean;
};

type Listener = (host: string, snapshot: TransferSnapshot | null) => void;

const listeners = new Set<Listener>();

/** Publish the current snapshot for a host (`null` = nothing in flight). */
export function publishTransfer(host: string, snapshot: TransferSnapshot | null): void {
  for (const listener of listeners) listener(host, snapshot);
}

/** Subscribe to every transfer snapshot; returns an unsubscribe function. */
export function subscribeTransfers(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
