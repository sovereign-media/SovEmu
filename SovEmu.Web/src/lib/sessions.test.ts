import { describe, expect, it } from "vitest";
import {
  clientSummary,
  decoderSummary,
  durationLabel,
  failureLabel,
  frameWorkLabel,
  healthSummary,
  playerLinkLabel,
  streamEventsLabel,
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
    encoded: 600,
    unsent: 0,
    keyframes: 0,
    keyframeRequests: 0,
    keyframeRequestsIgnored: 0,
    dropouts: 0,
    largestFrame: 9000,
  };

  it("says what happened to the stream only when something did", () => {
    expect(streamEventsLabel(health)).toBeNull();
    expect(
      streamEventsLabel({
        ...health,
        keyframes: 2,
        keyframeRequests: 5,
        keyframeRequestsIgnored: 3,
        dropouts: 1,
        unsent: 4,
        largestFrame: 38_900,
      }),
    ).toBe("2 keyframes (5 asked, 3 ignored) · 1 dropout · 4 of 600 frames not sent · largest 38 KB");
    expect(streamEventsLabel({ ...health, keyframes: 1, keyframeRequests: 1 })).toBe(
      "1 keyframe (1 asked) · largest 9 KB",
    );
  });

  it("says how the player reaches the stream", () => {
    const link = {
      estimateMs: null,
      inputToFrameMs: null,
      netRttMs: null,
      decodeMs: null,
      fps: null,
      kbps: null,
      decoder: null,
      lowLatencyDecode: null,
      path: "relay-tcp",
      width: 1280,
      height: 720,
      inputRttMs: 24,
      inputRttP95Ms: 61,
      packetsLost: 12,
      framesDropped: 3,
    };
    expect(playerLinkLabel({ clientStats: link })).toBe(
      "relay-tcp · 1280×720 · input round trip 24 / 61 ms (p95) · 12 lost · 3 dropped",
    );
    expect(playerLinkLabel({ clientStats: null })).toBeNull();
  });

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
      inputRttP95Ms: null as number | null,
      framesDropped: null as number | null,
      width: null as number | null,
      height: null as number | null,
      path: null as string | null,
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

  it("says how a game failed, and when", () => {
    const f = { whilePlaying: true, reason: "crashed", exitCode: null, signal: 11 };
    expect(failureLabel(f)).toBe("Crashed while playing (SIGSEGV)");
    expect(failureLabel({ ...f, signal: 31 })).toBe("Crashed while playing (signal 31)");
    expect(failureLabel({ ...f, whilePlaying: false, reason: "error", signal: null, exitCode: 1 })).toBe(
      "Failed to start (exit 1)",
    );
    expect(failureLabel({ ...f, whilePlaying: false, reason: "killed", signal: 9 })).toBe(
      "Timed out starting (SIGKILL)",
    );
    expect(failureLabel({ ...f, reason: "killed", signal: null })).toBe("Would not stop");
    expect(failureLabel({ ...f, whilePlaying: false, reason: "refused", signal: null })).toBe("Refused to start");
    // The message says which limit; the signal adds nothing.
    expect(failureLabel({ ...f, reason: "over_limit", signal: 6 })).toBe("Over its limit while playing");
  });
});
