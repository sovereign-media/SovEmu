// A pad drawn from the server's layout data (docs/CONTROLLERS.md, "Systems and diagrams"): the
// body's rounded rectangles, then each button with its label, the highlighted ones in the
// accent colour. The same data draws the console's pad and the player's controller, so the two
// can sit side by side with the button being remapped lit on both.
import type { PadLayout } from "../lib/api";
import { cn } from "./ui/cn";

export function PadDiagram({
  layout,
  highlight = [],
  color = "#4169e1",
  caption,
  className,
}: {
  layout: PadLayout;
  /** Keys to light up: RetroPad buttons on a system's pad, inputs on a controller's. */
  highlight?: string[];
  color?: string;
  caption?: string;
  className?: string;
}) {
  const lit = new Set(highlight);
  // Crop to what is drawn, so a flat pad does not float in an empty canvas.
  const shapes = [...layout.body, ...layout.keys];
  const pad = 3;
  const x0 = Math.max(0, Math.min(...shapes.map((s) => s.x)) - pad);
  const y0 = Math.max(0, Math.min(...shapes.map((s) => s.y)) - pad);
  const x1 = Math.min(layout.width, Math.max(...shapes.map((s) => s.x + s.w)) + pad);
  const y1 = Math.min(layout.height, Math.max(...shapes.map((s) => s.y + s.h)) + pad);
  return (
    <figure className={cn("grid gap-1", className)}>
      <svg viewBox={shapes.length ? `${x0} ${y0} ${x1 - x0} ${y1 - y0}` : `0 0 ${layout.width} ${layout.height}`} role="img" aria-label={caption ?? layout.name} className="mx-auto max-h-56 w-full">
        {layout.body.map((s, i) => (
          <rect key={i} x={s.x} y={s.y} width={s.w} height={s.h} rx={s.r} className={s.tone ? "fill-black/45" : "fill-slate-700"} />
        ))}
        {layout.keys.map((k) => {
          const on = lit.has(k.key);
          // Small keys get small labels; long labels ("Start") shrink to fit.
          const size = Math.min(k.h * 0.62, (k.w * 1.5) / Math.max(1, k.label.length), 6);
          return (
            <g key={k.key}>
              <rect
                x={k.x}
                y={k.y}
                width={k.w}
                height={k.h}
                rx={k.r}
                className={cn("transition-colors", on ? "" : "fill-slate-500")}
                style={on ? { fill: color } : undefined}
                stroke={on ? "white" : "none"}
                strokeWidth={on ? 0.8 : 0}
              />
              <text
                x={k.x + k.w / 2}
                y={k.y + k.h / 2}
                textAnchor="middle"
                dominantBaseline="central"
                fontSize={size}
                className={cn("pointer-events-none font-bold select-none", on ? "fill-white" : "fill-slate-900")}
              >
                {k.label}
              </text>
            </g>
          );
        })}
      </svg>
      {caption && <figcaption className="text-center text-xs text-slate-400">{caption}</figcaption>}
    </figure>
  );
}
