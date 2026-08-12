export interface PaintScheduler {
  schedule(): void;
  dispose(): void;
}

export interface PaintSchedulerOptions {
  shouldPaint(): boolean;
  paint(): void;
  requestFrame?: (callback: FrameRequestCallback) => number;
  cancelFrame?: (handle: number) => void;
}

/** Coalesce high-frequency terminal events into at most one paint per frame. */
export function createPaintScheduler(options: PaintSchedulerOptions): PaintScheduler {
  const requestFrame = options.requestFrame ?? ((callback) => requestAnimationFrame(callback));
  const cancelFrame = options.cancelFrame ?? ((handle) => cancelAnimationFrame(handle));
  let frame: number | null = null;
  let disposed = false;

  return {
    schedule() {
      if (disposed || frame !== null || !options.shouldPaint()) return;
      frame = requestFrame(() => {
        frame = null;
        if (!disposed && options.shouldPaint()) options.paint();
      });
    },
    dispose() {
      disposed = true;
      if (frame === null) return;
      cancelFrame(frame);
      frame = null;
    },
  };
}
