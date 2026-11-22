import { describe, expect, it } from "vitest";
import {
  clientSummary,
  decoderSummary,
  durationLabel,
  failureSummary,
  frameWorkLabel,
  healthSummary,
} from "./sessions";

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
    coreMeanMs: 24.06,
    readbackMeanMs: 6.3,
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

  it("splits a frame's time into the core, the GPU readback and encoding", () => {
    expect(frameWorkLabel(health)).toBe("core 24.1 ms · GPU readback 6.3 ms · encode 1.7 ms");
    expect(frameWorkLabel({ ...health, readbackMeanMs: 0 })).toBe("core 24.1 ms · encode 1.7 ms");
  });

  it("shows the player's view only while their panel is open", () => {
    const stats = {
      estimateMs: 44.6,
      inputToFrameMs: null as number | null,
      decoder: null as string | null,
      lowLatencyDecode: null as boolean | null,
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
    expect(decoderSummary({ clientStats: stats })).toBeNull();
    expect(decoderSummary({ clientStats: { ...stats, decoder: "ExternalDecoder" } })).toBe("ExternalDecoder");
    expect(
      decoderSummary({ clientStats: { ...stats, decoder: "c2.amlogic.avc.decoder", lowLatencyDecode: false } }),
    ).toBe("c2.amlogic.avc.decoder · low latency: not supported");
    expect(decoderSummary({ clientStats: { ...stats, decoder: "c2.qti.avc.decoder", lowLatencyDecode: true } })).toBe(
      "c2.qti.avc.decoder · low latency: on",
    );
  });
});

describe("session failures", () => {
  const failure = { phase: "RUNNING", reason: null, message: null, exitCode: null, signal: null } as const;

  it("says why the server refused a start, or why a start failed", () => {
    const refused = { ...failure, phase: "STARTING", message: "no core installed for PlayStation" } as const;
    expect(failureSummary(refused)).toBe("no core installed for PlayStation");
    const failed = { ...failure, phase: "LOADING", reason: "ERROR", message: "no BIOS", exitCode: 1 } as const;
    expect(failureSummary(failed)).toBe("no BIOS");
  });

  it("names the signal a crash died of", () => {
    expect(failureSummary({ ...failure, reason: "CRASHED", signal: 11 })).toBe(
      "Crashed: segmentation fault (signal 11)",
    );
    expect(failureSummary({ ...failure, reason: "CRASHED", signal: 31 })).toBe("Crashed: signal 31");
    expect(failureSummary({ ...failure, reason: "CRASHED", message: "ignored" })).toBe("Crashed: killed by a signal");
  });

  it("tells a slow load from a worker that would not stop", () => {
    expect(failureSummary({ ...failure, phase: "LOADING", reason: "KILLED" })).toBe(
      "Took too long to load, and was stopped",
    );
    expect(failureSummary({ ...failure, reason: "KILLED" })).toBe("Did not stop when asked, and was killed");
    expect(failureSummary({ ...failure, reason: "ERROR", exitCode: 2 })).toBe("Failed (exit code 2)");
    expect(failureSummary({ ...failure, phase: "LOADING", reason: "EXITED" })).toBe("Ended while loading");
  });
});
