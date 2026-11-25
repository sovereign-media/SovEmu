import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Session } from "./session";
import { StatsSampler } from "./stats";

/** A session whose `getStats` answers with the reports given, one per sample. */
function session(reports: Record<string, unknown>[][], inputRtts: number[] = [20, 30, 40]): Session {
  let i = 0;
  const pc = {
    getStats: async () => {
      const report = new Map((reports[Math.min(i++, reports.length - 1)] ?? []).map((s, n) => [String(n), s]));
      return report;
    },
  };
  return { pc, inputRtts, latency: { medianMs: 55 }, pipeline: null } as unknown as Session;
}

const video = (over: Record<string, number | string> = {}) => ({
  type: "inbound-rtp",
  kind: "video",
  bytesReceived: 0,
  jitterBufferDelay: 0,
  jitterBufferEmittedCount: 0,
  framesDecoded: 0,
  totalDecodeTime: 0,
  frameWidth: 256,
  frameHeight: 240,
  framesPerSecond: 60,
  packetsLost: 0,
  ...over,
});

describe("StatsSampler", () => {
  let now = 0;
  beforeEach(() => {
    now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
  });
  afterEach(() => vi.restoreAllMocks());

  it("shows nothing until the video has a report", async () => {
    expect(await new StatsSampler().sample(session([[]]))).toEqual({ items: [], numbers: {} });
  });

  it("works rates out between two samples, not from the first alone", async () => {
    const sampler = new StatsSampler();
    const s = session([
      [video()],
      // One second later: 125 kB, 60 frames out of the jitter buffer that held them 0.6 s in
      // all, 60 decoded in 0.3 s.
      [
        video({
          bytesReceived: 125_000,
          jitterBufferDelay: 0.6,
          jitterBufferEmittedCount: 60,
          framesDecoded: 60,
          totalDecodeTime: 0.3,
          packetsLost: 2,
        }),
        { type: "candidate-pair", nominated: true, state: "succeeded", currentRoundTripTime: 0.004 },
      ],
    ]);
    // The first sample has nothing to compare with: a rate would be a guess, so it is "unknown".
    const first = await sampler.sample(s);
    expect(Number.isNaN(first.numbers.kbps)).toBe(true);
    expect(first.items.find((i) => i.label === "Bitrate")?.value).toBe("–");

    now = 1000;
    const { numbers, items } = await sampler.sample(s);
    expect(numbers.kbps).toBe(1000);
    expect(numbers.jitter_ms).toBeCloseTo(10);
    expect(numbers.decode_ms).toBeCloseTo(5);
    expect(numbers.net_rtt_ms).toBeCloseTo(4);
    expect(numbers.packets_lost).toBe(2);
    // The input round trip is the median of what the acks measured; p95 the nearest rank.
    expect(numbers.input_rtt_p50_ms).toBe(30);
    expect(numbers.input_rtt_p95_ms).toBe(40);
    // Button to pixel: the input round trip, half a frame, encode, jitter, decode, half a refresh.
    expect(numbers.estimate_ms).toBeCloseTo(30 + 8 + 2 + 10 + 5 + 8);
    expect(items.find((i) => i.label === "Video")?.value).toBe("256×240 @ 60 fps");
    expect(items.find((i) => i.label === "Network RTT")?.value).toBe("4.0 ms");
  });

  it("leaves out the jitter and decode it cannot work out, rather than dividing by zero", async () => {
    const sampler = new StatsSampler();
    const s = session([[video()], [video({ bytesReceived: 1000 })]]);
    await sampler.sample(s);
    now = 500;
    const { numbers, items } = await sampler.sample(s);
    expect(numbers.kbps).toBe(16);
    expect(Number.isNaN(numbers.jitter_ms)).toBe(true);
    expect(items.find((i) => i.label === "Decode")?.value).toBe("–");
    // The estimate still stands, without the parts nobody measured.
    expect(numbers.estimate_ms).toBeCloseTo(30 + 8 + 2 + 8);
  });
});
