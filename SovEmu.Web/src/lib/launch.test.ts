import { describe, expect, it } from "vitest";
import { describeFirstPicture, describeStart, ForwardOnly, newStartId } from "./launch";

const status = (over: object) =>
  ({ phase: "CONTENT", elapsedMs: 0, phaseMs: 0, ...over }) as Parameters<typeof describeStart>[0];

describe("the loading bar", () => {
  it("is exact when the total is known", () => {
    const half = describeStart(status({ read: 32 * 1024 * 1024, total: 64 * 1024 * 1024, elapsedMs: 5000 }));
    expect(half.label).toBe("Loading the game");
    expect(half.detail).toBe("32 of 64 MB · 5 s");
    expect(half.fraction).toBeCloseTo(0.18 + 0.79 / 2);
  });

  it("eases forward and counts bytes when the total is not known", () => {
    const early = describeStart(status({ read: 3 * 1024 * 1024, phaseMs: 1000 }));
    const later = describeStart(status({ read: 250 * 1024 * 1024, phaseMs: 30000 }));
    expect(early.detail).toBe("3 MB read");
    expect(later.detail).toBe("250 MB read");
    expect(later.fraction).toBeGreaterThan(early.fraction);
    expect(later.fraction).toBeLessThan(0.97);
  });

  it("names every phase and reports failure", () => {
    expect(describeStart(status({ phase: "STOPPING" })).label).toBe("Saving your last game");
    expect(describeStart(status({ phase: "CORE" })).fraction).toBeCloseTo(0.12);
    expect(describeStart(status({ phase: "READY" })).fraction).toBe(1);
    const failed = describeStart(status({ phase: "FAILED", error: "no core installed" }));
    expect([failed.label, failed.detail]).toEqual(["Could not start", "no core installed"]);
  });

  it("never moves backwards", () => {
    const bar = new ForwardOnly();
    expect(bar.next(0.5)).toBe(0.5);
    expect(bar.next(0.3)).toBe(0.5);
    expect(bar.next(2)).toBe(1);
  });

  it("makes ids the server accepts", () => {
    expect(newStartId()).toMatch(/^[A-Za-z0-9_-]{8,64}$/);
  });
});

describe("waiting for the first picture", () => {
  it("counts the wait, and says how to leave a game that never draws", () => {
    expect(describeFirstPicture(500)).toEqual({ label: "Starting the game", detail: "", fraction: 1 });
    expect(describeFirstPicture(6200).detail).toBe("6 s");
    expect(describeFirstPicture(21_000).detail).toBe("21 s · still no picture: Escape opens the menu");
  });
});
