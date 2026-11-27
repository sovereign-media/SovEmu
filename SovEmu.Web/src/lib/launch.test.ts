import { describe, expect, it } from "vitest";
import {
  describeConnecting,
  describeFirstPicture,
  describeLink,
  describeStart,
  ForwardOnly,
  loadingBackground,
  newStartId,
} from "./launch";
import type { Link } from "./protocol";

const status = (over: object) =>
  ({ phase: "CONTENT", elapsedMs: 0, phaseMs: 0, ...over }) as Parameters<typeof describeStart>[0];

describe("the loading bar", () => {
  it("is exact when the total is known", () => {
    const half = describeStart(status({ read: 32 * 1024 * 1024, total: 64 * 1024 * 1024, elapsedMs: 5000 }));
    expect(half.label).toBe("Loading the game");
    expect(half.detail).toBe("32 of 64 MB · 5 s");
    expect(half.fraction).toBeCloseTo(0.18 + 0.67 / 2);
  });

  it("eases forward and counts bytes when the total is not known", () => {
    const early = describeStart(status({ read: 3 * 1024 * 1024, phaseMs: 1000 }));
    const later = describeStart(status({ read: 250 * 1024 * 1024, phaseMs: 30000 }));
    expect(early.detail).toBe("3 MB read");
    expect(later.detail).toBe("250 MB read");
    expect(later.fraction).toBeGreaterThan(early.fraction);
    expect(later.fraction).toBeLessThan(0.85);
  });

  it("names every phase and reports failure", () => {
    expect(describeStart(status({ phase: "STOPPING" })).label).toBe("Saving your last game");
    expect(describeStart(status({ phase: "CORE" })).fraction).toBeCloseTo(0.12);
    // Ready leaves the bar's last stretch to checking the connection and the first picture.
    expect(describeStart(status({ phase: "READY" })).fraction).toBe(0.85);
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
    expect(describeFirstPicture(500)).toEqual({ label: "Starting the game", detail: "", fraction: 0.97 });
    expect(describeFirstPicture(6200).detail).toBe("6 s");
    expect(describeFirstPicture(21_000).detail).toBe("21 s · still no picture: Escape opens the menu");
  });
});

describe("checking the connection", () => {
  const link = (over: Partial<Link> = {}): Link => ({
    state: "probing",
    elapsedMs: 0,
    budgetMs: 2000,
    path: "",
    estimateKbps: 0,
    startKbps: 0,
    rttMs: null,
    stoppedBy: "",
    ...over,
  });

  it("moves the bar from ready to almost full over the check's budget", () => {
    // The worker sends no video while it checks: without this stage the bar would sit at
    // "Ready" for up to two seconds with nothing said.
    expect(describeLink(link()).fraction).toBeCloseTo(0.85);
    expect(describeLink(link({ elapsedMs: 1000 })).fraction).toBeCloseTo(0.91);
    expect(describeLink(link({ elapsedMs: 5000 })).fraction).toBeCloseTo(0.97);
    expect(describeLink(link({ budgetMs: 0 })).fraction).toBeCloseTo(0.97);
    expect(describeLink(link()).label).toBe("Checking your connection");
  });

  it("says it is connecting until the worker is reached", () => {
    // Otherwise the screen read "Starting the game" for a second or two before it went back to
    // checking the connection.
    expect(describeConnecting(500)).toEqual({ label: "Connecting", detail: "", fraction: 0.85 });
    expect(describeConnecting(4200).detail).toBe("4 s");
  });

  it("leaves where the player is to the network chip", () => {
    // The chip in the corner says Home network or Remote; the bar's detail stays free for the wait.
    expect(describeLink(link({ path: "lan" })).detail).toBe("");
    expect(describeFirstPicture(4000).detail).toBe("4 s");
  });
});

describe("the loading screen's background", () => {
  it("shows where the player left off, else the game, else its box blurred", () => {
    // Continuing: the save's own screenshot, so the player sees where they will be.
    expect(loadingBackground("save.png", "shot.webp", "box.webp")).toEqual({
      url: "save.png",
      pixelated: true,
      blurred: false,
    });
    expect(loadingBackground(null, "shot.webp", "box.webp")).toEqual({
      url: "shot.webp",
      pixelated: false,
      blurred: false,
    });
    expect(loadingBackground(undefined, null, "box.webp")).toEqual({
      url: "box.webp",
      pixelated: false,
      blurred: true,
    });
    expect(loadingBackground(null, null, null)).toBeNull();
  });
});
