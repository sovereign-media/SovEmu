import { describe, expect, it, vi } from "vitest";
import { clientName, MAX_BATCH, Reporter } from "./clientEvents";

describe("client events", () => {
  it("gathers events, sends each once, and splits a burst into batches", () => {
    vi.useFakeTimers();
    const sent: number[] = [];
    const r = new Reporter(
      (events) => sent.push(events.length),
      () => 1,
    );
    r.report("CONNECTION_FAILED", "ICE failed", "abc");
    r.report("CONNECTION_FAILED", "ICE failed", "abc"); // the same fault again: not resent
    expect(sent).toEqual([]);
    vi.advanceTimersByTime(2000);
    expect(sent).toEqual([1]);
    for (let i = 0; i < MAX_BATCH + 5; i++) r.report("OTHER", `event ${i}`);
    r.flush();
    expect(sent).toEqual([1, MAX_BATCH, 5]);
    vi.useRealTimers();
  });

  it("names the browser and the system", () => {
    const chrome =
      "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
    expect(clientName(chrome)).toBe("web (Chrome 131, Linux)");
    const safari =
      "Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1";
    expect(clientName(safari)).toBe("web (Safari 17, iPad)");
    expect(clientName("")).toBe("web");
  });
});
