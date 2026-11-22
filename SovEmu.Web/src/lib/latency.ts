// Measured button-to-frame latency, and the history the stats panel's graph draws.
//
// The worker acks each input just before the video of the frame that consumed it, so the first
// frame to *arrive* after an ack is the one showing that input: from sending the input to that
// frame reaching the screen is button to frame, measured (decoding, and any frames the decoder
// holds back, included). The display's own lag comes on top. A frame that arrived before the ack
// but is shown after it (a decoder running a frame or two behind) is an older picture and must
// not count, so each frame is matched by when it was received, not by when it is shown. The
// Android app measures the same way.

import { median } from "./numbers";

/** How many recent inputs the median is taken over. */
const SAMPLES = 60;

/**
 * The ack leaves the worker just before its frame, but its arrival is timed on the page's main
 * thread while the frame's is timed on the network thread, so the frame can look a few
 * milliseconds earlier than its ack. Half a 60 fps frame of slack lets it through without
 * letting through the frame before it (a whole frame earlier).
 */
const ACK_SLACK_MS = 8;

/** Times inputs from being sent to the first frame shown after their ack. */
export class LatencyMeter {
  /** The input whose ack came last (when it was sent, when the ack arrived), until its frame is shown. */
  private pending: { sentAt: number; ackedAt: number } | null = null;
  private samples: number[] = [];

  /** An ack arrived at `ackedAt` for an input sent at `sentAt` (the same clock as `frameShown`'s). */
  acked(sentAt: number, ackedAt: number) {
    // Keep the oldest waiting input: a newer ack's frame comes later still.
    if (this.pending === null) this.pending = { sentAt, ackedAt };
  }

  /**
   * A frame reached the screen at `at`, having been received at `receivedAt`. It shows the
   * waiting input only if it arrived after that input's ack; where the browser does not say when
   * a frame was received, the first frame shown after the ack is taken (low when decoding is
   * slower than a frame).
   */
  frameShown(at: number, receivedAt?: number) {
    if (this.pending === null) return;
    if (receivedAt !== undefined && receivedAt < this.pending.ackedAt - ACK_SLACK_MS) return;
    const ms = at - this.pending.sentAt;
    this.pending = null;
    // A clock jump or a frame from before the ack: not a measurement.
    if (ms < 0 || ms > 2000) return;
    this.samples.push(ms);
    if (this.samples.length > SAMPLES) this.samples.shift();
  }

  /** The median of recent measurements, NaN before the first. */
  get medianMs(): number {
    return median(this.samples);
  }
}

/** One second of the graph: measured and estimated latency, ms (NaN where unknown). */
export interface LatencyPoint {
  measured: number;
  estimate: number;
}

/** The graph's last minute, one point a second. */
export class LatencyHistory {
  readonly points: LatencyPoint[] = [];

  constructor(readonly capacity = 60) {}

  push(point: LatencyPoint) {
    this.points.push(point);
    if (this.points.length > this.capacity) this.points.shift();
  }

  /** The y axis's top: the next multiple of 25 ms above every value shown, at least 50 ms. */
  get scaleMax(): number {
    const values = this.points.flatMap((p) => [p.measured, p.estimate]).filter(Number.isFinite);
    const top = values.length ? Math.max(...values) : 0;
    return Math.max(50, Math.ceil((top + 1) / 25) * 25);
  }
}

/**
 * Splits rows into two columns, the first filled before the second (`[1, 2, 3, 4, 5]` → rows
 * `[1, 4]`, `[2, 5]`, `[3]`), keeping their order top to bottom then left to right.
 */
export function twoColumns<T>(items: T[]): [T, T | undefined][] {
  const half = Math.ceil(items.length / 2);
  return items.slice(0, half).map((item, i) => [item, items[half + i]]);
}
