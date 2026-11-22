// The admin Resources page's arithmetic: sizes, how worrying a figure is, and the geometry of the
// CPU and memory charts (a line for each bucket's average over a faint area up to its peak,
// broken where the server was down). Times from the API are Unix seconds; figures are 0–100.

export interface Point {
  at: number;
  average: number;
  max: number;
}

/** "2 cores", "1.5 cores". */
export function formatCores(cores: number): string {
  const n = Number.isInteger(cores) ? String(cores) : cores.toFixed(1);
  return `${n} ${cores === 1 ? "core" : "cores"}`;
}

/** How a share of a limit reads: fine, high (over 80%) or at the limit (over 95%). The colour
 *  always comes with the label, never alone. */
export function severity(percent: number): { label: string; color: string } {
  if (percent > 95) return { label: "At the limit", color: "#ef4444" };
  if (percent > 80) return { label: "High", color: "#f59e0b" };
  return { label: "", color: "#4169e1" };
}

/** Points split where buckets are missing: each run is drawn on its own, so a gap shows as one. */
export function runs(points: Point[], bucketSeconds: number): Point[][] {
  const out: Point[][] = [];
  for (const p of points) {
    const run = out[out.length - 1];
    const last = run?.[run.length - 1];
    if (last && p.at - last.at <= bucketSeconds * 1.5) run.push(p);
    else out.push([p]);
  }
  return out;
}

export interface Geometry {
  /** The averages: one path, a move between runs. */
  line: string;
  /** The peaks: a closed area per run, down to the baseline. */
  area: string;
  /** Where a lone point (a run of one) sits, drawn as a dot. */
  dots: { x: number; y: number }[];
}

/** The chart's paths over a `width` × `height` plot of the window `from`–`to`, 0–100 high. A
 *  point sits at its bucket's middle. */
export function geometry(
  points: Point[],
  window: { from: number; to: number; bucketSeconds: number },
  width: number,
  height: number,
): Geometry {
  const span = Math.max(1, window.to - window.from);
  const x = (at: number) => round(((at + window.bucketSeconds / 2 - window.from) / span) * width);
  const y = (v: number) => round(height - (Math.min(100, Math.max(0, v)) / 100) * height);
  let line = "";
  let area = "";
  const dots: { x: number; y: number }[] = [];
  for (const run of runs(points, window.bucketSeconds)) {
    if (run.length === 1) {
      dots.push({ x: x(run[0].at), y: y(run[0].average) });
      continue;
    }
    line += run.map((p, i) => `${i ? "L" : "M"}${x(p.at)},${y(p.average)}`).join("");
    const top = run.map((p, i) => `${i ? "L" : "M"}${x(p.at)},${y(p.max)}`).join("");
    area += `${top}L${x(run[run.length - 1].at)},${height}L${x(run[0].at)},${height}Z`;
  }
  return { line, area, dots };
}

function round(n: number): number {
  return Math.round(n * 10) / 10;
}

/** About five evenly spaced times across the window, on whole steps of a sensible size. */
export function timeTicks(from: number, to: number): number[] {
  const steps = [600, 900, 1800, 3600, 3 * 3600, 6 * 3600, 12 * 3600, 86400, 2 * 86400, 7 * 86400];
  const step = steps.find((s) => (to - from) / s <= 5) ?? 7 * 86400;
  const ticks: number[] = [];
  for (let t = Math.ceil(from / step) * step; t <= to; t += step) ticks.push(t);
  return ticks;
}

/** How a tick reads: the time of day for a window under two days, else the date. */
export function tickLabel(at: number, windowSeconds: number): string {
  const d = new Date(at * 1000);
  return windowSeconds <= 2 * 86400
    ? d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
    : d.toLocaleDateString([], { month: "short", day: "numeric" });
}

/** The point nearest `at`, for the hover readout. */
export function nearest(points: Point[], at: number): Point | null {
  let best: Point | null = null;
  for (const p of points) if (!best || Math.abs(p.at - at) < Math.abs(best.at - at)) best = p;
  return best;
}
