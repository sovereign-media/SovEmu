import { afterEach, describe, expect, it, vi } from "vitest";
import { startTicker } from "./ticker";

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("startTicker", () => {
  it("ticks on a plain timer where there are no workers, until stopped", () => {
    vi.useFakeTimers();
    vi.stubGlobal("Worker", undefined);
    const tick = vi.fn();
    const stop = startTicker(100, tick);
    vi.advanceTimersByTime(350);
    expect(tick).toHaveBeenCalledTimes(3);
    stop();
    vi.advanceTimersByTime(500);
    expect(tick).toHaveBeenCalledTimes(3);
  });

  it("ticks from a worker's messages where there is one, and ends it when stopped", () => {
    const workers: { onmessage: (() => void) | null; terminate: () => void; url: string }[] = [];
    class FakeWorker {
      onmessage: (() => void) | null = null;
      terminate = vi.fn();
      constructor(readonly url: string) {
        workers.push(this);
      }
    }
    vi.stubGlobal("Worker", FakeWorker);
    const tick = vi.fn();
    const stop = startTicker(100, tick);
    expect(workers).toHaveLength(1);
    workers[0].onmessage?.();
    workers[0].onmessage?.();
    expect(tick).toHaveBeenCalledTimes(2);
    stop();
    expect(workers[0].terminate).toHaveBeenCalled();
  });
});
