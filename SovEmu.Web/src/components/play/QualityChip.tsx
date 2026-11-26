// The notice of a change in the stream's quality: Sovereign's player message chip (its
// `PlayerMessageChip`: a glyph before one line, gray-950 at 75% under a 2 px inset white/40 ring,
// which reads on a bright picture and on black alike), but at the very top edge of the screen
// rather than 12% down, so it stays out of the game.
import { Gauge } from "lucide-react";
import type { ReactNode } from "react";
import type { QualityNotice } from "../../lib/pipeline";

type WifiGlyphProps = { children: ReactNode };

/** Sovereign.Design's own `wifi-alert` and `wifi-up` (`icons/own/`), drawn in Lucide's style. */
function WifiGlyph({ children }: WifiGlyphProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className="size-5 shrink-0"
      aria-hidden="true"
    >
      <path d="M7.5 15.5h.01" />
      <path d="M1.5 8.8a9 9 0 0 1 12 0" />
      <path d="M3.3 11.2a6 6 0 0 1 8.4 0" />
      <path d="M5.4 13.4a3 3 0 0 1 4.2 0" />
      {children}
    </svg>
  );
}

const GLYPHS: Record<QualityNotice["icon"], ReactNode> = {
  "wifi-alert": (
    <WifiGlyph>
      <path d="M16.75 9.5l6.25 12h-12.5z" />
      <path d="M16.75 13.75v3" />
      <path d="M16.75 19.25h.01" />
    </WifiGlyph>
  ),
  "wifi-up": (
    <WifiGlyph>
      <path d="M17 21.5v-11" />
      <path d="M13 14.5l4-4 4 4" />
    </WifiGlyph>
  ),
  gauge: <Gauge className="size-5 shrink-0" aria-hidden="true" />,
};

type QualityChipProps = { notice: QualityNotice };

/** Inert, so it never takes a press meant for the game. */
export function QualityChip({ notice }: QualityChipProps) {
  return (
    <div className="pointer-events-none absolute top-2 left-1/2 z-10 flex max-w-[calc(100%-2rem)] -translate-x-1/2">
      <output className="flex h-10 min-w-0 items-center gap-2 rounded-full bg-gray-950/75 px-4 text-base text-white ring-2 ring-white/40 ring-inset">
        {GLYPHS[notice.icon]}
        <p className="min-w-0 truncate">{notice.line}</p>
      </output>
    </div>
  );
}
