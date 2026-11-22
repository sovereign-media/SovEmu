// The stats panel's live latency graph: the last minute of measured button→frame (solid) and the
// estimate (faint), redrawn once a second as the stats are sampled. The lines stretch to the
// panel's width (a 100×100 box, non-scaling strokes); the labels are text at its real size.

import type { LatencyPoint } from "../../lib/latency";

/** The share of the width left for the latest value on the right. */
const PLOT = 88;

function path(points: LatencyPoint[], key: keyof LatencyPoint, capacity: number, max: number): string {
  const step = PLOT / Math.max(1, capacity - 1);
  const start = capacity - points.length;
  let d = "";
  let pen = false;
  points.forEach((p, i) => {
    const v = p[key];
    if (!Number.isFinite(v)) {
      pen = false;
      return;
    }
    const x = (start + i) * step;
    const y = 100 - (Math.min(v, max) / max) * 100;
    d += `${pen ? "L" : "M"}${x.toFixed(2)},${y.toFixed(2)}`;
    pen = true;
  });
  return d;
}

export function LatencyGraph({ points, capacity, max }: { points: LatencyPoint[]; capacity: number; max: number }) {
  const latest = [...points].reverse().find((p) => Number.isFinite(p.measured))?.measured;
  const top = (v: number) => `${100 - (Math.min(v, max) / max) * 100}%`;
  return (
    <div
      className="relative mt-2 h-14"
      role="img"
      aria-label={`Latency over the last minute${latest !== undefined ? `, now ${Math.round(latest)} ms measured` : ""}`}
    >
      <svg
        viewBox="0 0 100 100"
        preserveAspectRatio="none"
        className="absolute inset-0 size-full overflow-visible"
        aria-hidden="true"
      >
        {[max / 2, max].map((g) => (
          <line
            key={g}
            x1={0}
            x2={PLOT}
            y1={100 - (g / max) * 100}
            y2={100 - (g / max) * 100}
            className="stroke-white/10"
            vectorEffect="non-scaling-stroke"
          />
        ))}
        <line x1={0} x2={PLOT} y1={100} y2={100} className="stroke-white/25" vectorEffect="non-scaling-stroke" />
        <path
          d={path(points, "estimate", capacity, max)}
          fill="none"
          className="stroke-slate-400/60"
          strokeWidth={1}
          vectorEffect="non-scaling-stroke"
        />
        <path
          d={path(points, "measured", capacity, max)}
          fill="none"
          className="stroke-sky-400"
          strokeWidth={2}
          vectorEffect="non-scaling-stroke"
        />
      </svg>
      {[max / 2, max].map((g) => (
        <span key={g} className="absolute left-0.5 text-[9px] leading-none text-slate-500" style={{ top: top(g) }}>
          {g} ms
        </span>
      ))}
      {latest !== undefined && (
        <span
          className="absolute right-0 -translate-y-1/2 text-[10px] leading-none text-sky-300 tabular-nums"
          style={{ top: top(latest) }}
        >
          {Math.round(latest)} ms
        </span>
      )}
    </div>
  );
}
