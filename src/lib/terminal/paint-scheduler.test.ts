import { describe, expect, it, vi } from "vitest";
import { createPaintScheduler } from "./paint-scheduler";

function setup() {
  let callback: FrameRequestCallback | null = null;
  let shouldPaint = true;
  const paint = vi.fn();
  const cancelFrame = vi.fn();
  const requestFrame = vi.fn((next: FrameRequestCallback) => {
    callback = next;
    return 42;
  });
  const scheduler = createPaintScheduler({
    shouldPaint: () => shouldPaint,
    paint,
    requestFrame,
    cancelFrame,
  });

  return {
    scheduler,
    paint,
    requestFrame,
    cancelFrame,
    setShouldPaint(value: boolean) {
      shouldPaint = value;
    },
    flush() {
      const pending = callback;
      callback = null;
      pending?.(0);
    },
  };
}

describe("createPaintScheduler", () => {
  it("coalesces repeated requests into one paint per frame", () => {
    const fixture = setup();
    fixture.scheduler.schedule();
    fixture.scheduler.schedule();
    fixture.scheduler.schedule();

    expect(fixture.requestFrame).toHaveBeenCalledTimes(1);
    fixture.flush();
    expect(fixture.paint).toHaveBeenCalledTimes(1);

    fixture.scheduler.schedule();
    fixture.flush();
    expect(fixture.paint).toHaveBeenCalledTimes(2);
  });

  it("checks whether painting is needed both before and inside the frame", () => {
    const fixture = setup();
    fixture.setShouldPaint(false);
    fixture.scheduler.schedule();
    expect(fixture.requestFrame).not.toHaveBeenCalled();

    fixture.setShouldPaint(true);
    fixture.scheduler.schedule();
    fixture.setShouldPaint(false);
    fixture.flush();
    expect(fixture.paint).not.toHaveBeenCalled();
  });

  it("cancels pending work and remains inert after disposal", () => {
    const fixture = setup();
    fixture.scheduler.schedule();
    fixture.scheduler.dispose();
    fixture.flush();
    fixture.scheduler.schedule();

    expect(fixture.cancelFrame).toHaveBeenCalledWith(42);
    expect(fixture.paint).not.toHaveBeenCalled();
    expect(fixture.requestFrame).toHaveBeenCalledTimes(1);
  });
});
