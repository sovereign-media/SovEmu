import { describe, expect, it } from "vitest";
import { formatCores, geometry, nearest, runs, severity, timeTicks } from "./resources";

const p = (at: number, average: number, max = average) => ({ at, average, max });

describe("resources", () => {
  it("says cores briefly", () => {
    expect(formatCores(2)).toBe("2 cores");
    expect(formatCores(1)).toBe("1 core");
    expect(formatCores(1.5)).toBe("1.5 cores");
  });

  it("labels a worrying figure, never only colours it", () => {
    expect(severity(50).label).toBe("");
    expect(severity(85).label).toBe("High");
    expect(severity(99).label).toBe("At the limit");
  });

  it("breaks a series where buckets are missing", () => {
    expect(runs([p(0, 1), p(10, 1), p(40, 1), p(50, 1)], 10).map((r) => r.length)).toEqual([2, 2]);
  });

  it("draws averages over peaks, with a gap and a lone point", () => {
    const g = geometry([p(0, 50, 100), p(10, 0, 50), p(90, 25)], { from: 0, to: 100, bucketSeconds: 10 }, 100, 100);
    // Points sit at their bucket's middle; 100% is the top.
    expect(g.line).toBe("M5,50L15,100");
    expect(g.area).toBe("M5,0L15,50L15,100L5,100Z");
    expect(g.dots).toEqual([{ x: 95, y: 75 }]);
  });

  it("puts about five ticks on whole steps", () => {
    expect(timeTicks(0, 3600)).toEqual([0, 900, 1800, 2700, 3600]);
    expect(timeTicks(100, 86400).length).toBeLessThanOrEqual(6);
  });

  it("finds the point nearest the pointer", () => {
    expect(nearest([p(0, 1), p(10, 2), p(20, 3)], 13)).toEqual(p(10, 2));
    expect(nearest([], 5)).toBeNull();
  });
});
