// The bottom bar of controller hints, as on a console menu: a button glyph and what it does.
// Face buttons are drawn as letters (no platform button artwork); the rest are Lucide icons.
import { Menu, Move, SquareMinus } from "lucide-react";
import type { ReactNode } from "react";

export type HintButton = "dpad" | "a" | "b" | "start" | "select";

const GLYPHS: Record<HintButton, ReactNode> = {
  dpad: <Move className="size-3.5" />,
  a: "A",
  b: "B",
  start: <Menu className="size-3.5" />,
  select: <SquareMinus className="size-3.5" />,
};

export default function HintBar({ hints }: { hints: { button: HintButton; label: string }[] }) {
  return (
    <footer className="pointer-events-none fixed inset-x-0 bottom-0 z-20 flex items-center justify-end gap-6 bg-gradient-to-t from-black/80 to-transparent px-8 pt-10 pb-5 text-sm text-slate-300">
      {hints.map((h) => (
        <span key={h.label} className="flex items-center gap-2">
          <span className="grid h-7 min-w-7 place-items-center rounded-full border border-white/30 bg-white/10 px-1.5 text-xs font-bold text-white">
            {GLYPHS[h.button]}
          </span>
          {h.label}
        </span>
      ))}
    </footer>
  );
}
