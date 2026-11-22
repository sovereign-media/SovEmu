// Measured button-to-frame latency, and the history the stats panel's graph draws.
//
// The worker acks each input just before the video of the frame that consumed it, so the first
// frame shown after an ack is the one showing that input: from sending the input to that frame
// is button to frame, measured (decoding, and any frames the decoder holds back, included). The
// display's own lag comes on top. The Android app measures the same way.

/** How many recent inputs the median is taken over. */
const SAMPLES = 60;

function median(values: number[]): number {
  if (!values.length) return NaN;
  const s = [...values].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
}

/** Times inputs from being sent to the first frame shown after their ack. */
export class LatencyMeter {
  /** The sent time of the input whose ack came last, until a frame is shown. */
  private pending: number | null = null;
  private samples: number[] = [];

  /** An ack arrived for an input sent at `sentAt` (the same clock as `frameShown`'s). */
  acked(sentAt: number) {
    this.pending = sentAt;
  }

  /** A frame reached the screen at `at`; it shows the input acked last, if one is waiting. */
  frameShown(at: number) {
    if (this.pending === null) return;
    const ms = at - this.pending;
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
