import { describe, expect, it } from "vitest";
import { clientSummary, durationLabel, healthSummary } from "./sessions";

describe("sessions page", () => {
  it("says how long a game has run", () => {
    expect(durationLabel(45.7)).toBe("45 s");
    expect(durationLabel(12 * 60 + 5)).toBe("12 min");
    expect(durationLabel(3600 + 5 * 60)).toBe("1 h 05 min");
    expect(durationLabel(-3)).toBe("0 s");
  });

  const health = {
    fps: 28.2,
    overran: 0.5,
    paused: 0,
    tickMeanMs: 31,
    encodeMeanMs: 1.74,
    videoKbps: 1820.4,
    videoKbpsTarget: 2500,
    estimateKbps: null,
  };

  it("sums up the worker's health against the game's frame rate", () => {
    const pipeline = { fps: 59.94 } as NonNullable<Parameters<typeof healthSummary>[0]["pipeline"]>;
    expect(healthSummary({ health, pipeline })).toEqual({
      text: "28.2 / 60 fps · 1,820 kbps · encode 1.7 ms",
      struggling: true,
    });
    expect(healthSummary({ health: { ...health, fps: 60, overran: 0 }, pipeline: null })).toEqual({
      text: "60.0 fps · 1,820 kbps · encode 1.7 ms",
      struggling: false,
    });
    expect(healthSummary({ health: { ...health, fps: 0, paused: 0.9 }, pipeline: null })).toEqual({
      text: "Paused (menu open)",
      struggling: false,
    });
    expect(healthSummary({ health: null, pipeline: null })).toBeNull();
  });

  it("shows the player's view only while their panel is open", () => {
    const stats = {
      estimateMs: 44.6,
      inputToFrameMs: null as number | null,
      inputRttMs: 22,
      netRttMs: 4,
      decodeMs: 3.14,
      fps: 60,
      kbps: 900,
      packetsLost: 0,
    };
    expect(clientSummary({ clientStats: stats })).toBe("~45 ms button→pixel · decode 3.1 ms");
    expect(clientSummary({ clientStats: { ...stats, packetsLost: 3 } })).toBe(
      "~45 ms button→pixel · decode 3.1 ms · 3 lost",
    );
    expect(clientSummary({ clientStats: { ...stats, inputToFrameMs: 38.6 } })).toBe(
      "39 ms button→frame · ~45 ms est. · decode 3.1 ms",
    );
    expect(clientSummary({ clientStats: null })).toBeNull();
  });
});
