import { describe, expect, it } from "vitest";
import { LatencyHistory, LatencyMeter, twoColumns } from "./latency";

describe("latency meter", () => {
  it("times each acked input to the next frame shown", () => {
    const meter = new LatencyMeter();
    expect(meter.medianMs).toBeNaN();
    meter.frameShown(5); // nothing acked yet
    meter.acked(100);
    meter.frameShown(140);
    meter.frameShown(160); // the next frame measures nothing more
    meter.acked(200);
    meter.acked(210); // a newer ack replaces the waiting one
    meter.frameShown(245);
    meter.acked(300);
    meter.frameShown(350);
    expect(meter.medianMs).toBe(40);
  });

  it("ignores impossible measurements", () => {
    const meter = new LatencyMeter();
    meter.acked(1000);
    meter.frameShown(900);
    meter.acked(0);
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
