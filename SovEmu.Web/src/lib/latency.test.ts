import { describe, expect, it } from "vitest";
import { LatencyHistory, LatencyMeter, twoColumns } from "./latency";

describe("latency meter", () => {
  it("times each acked input to the next frame shown", () => {
    const meter = new LatencyMeter();
    expect(meter.medianMs).toBeNaN();
    meter.frameShown(5); // nothing acked yet
    meter.acked(100, 120);
    meter.frameShown(140);
    meter.frameShown(160); // the next frame measures nothing more
    meter.acked(200, 220);
    meter.acked(210, 230); // the waiting input is kept: its frame comes first
    meter.frameShown(245);
    meter.acked(300, 320);
    meter.frameShown(350);
    expect(meter.medianMs).toBe(45);
  });

  it("skips frames that arrived before the ack: a slow decoder shows them after it", () => {
    const meter = new LatencyMeter();
    meter.acked(100, 125);
    meter.frameShown(130, 92); // received before the ack: an older picture
    meter.frameShown(147, 108); // and another, a frame before the ack
    expect(meter.medianMs).toBeNaN();
    meter.frameShown(163, 123); // timed a little before its ack (another thread), but its frame
    expect(meter.medianMs).toBe(63);
  });

  it("ignores impossible measurements", () => {
    const meter = new LatencyMeter();
    meter.acked(1000, 1010);
    meter.frameShown(900);
    meter.acked(0, 10);
    meter.frameShown(5000);
    expect(meter.medianMs).toBeNaN();
  });
});

describe("latency history", () => {
  it("keeps the last minute and scales to it", () => {
    const history = new LatencyHistory(3);
    expect(history.scaleMax).toBe(50);
    for (const measured of [30, 40, 61, 44]) history.push({ measured, estimate: 47 });
    expect(history.points.map((p) => p.measured)).toEqual([40, 61, 44]);
    expect(history.scaleMax).toBe(75);
    history.push({ measured: NaN, estimate: 75 });
    expect(history.scaleMax).toBe(100);
  });
});

describe("two columns", () => {
  it("fills the first column, then the second", () => {
    expect(twoColumns([1, 2, 3, 4, 5])).toEqual([
      [1, 4],
      [2, 5],
      [3, undefined],
    ]);
    expect(twoColumns([])).toEqual([]);
  });
});
