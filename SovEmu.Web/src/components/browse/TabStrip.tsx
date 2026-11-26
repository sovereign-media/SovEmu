// The browse header's capsule of chips, after Sovereign's TabStrip: actions (Sort, Filters,
// Jump) as pills in one rounded strip. A chip that says a filter is applied is quietly filled;
// it is never the current tab's accent.
import type { ReactNode } from "react";

const SHAPE =
  "flex h-8 shrink-0 cursor-pointer items-center gap-1.5 whitespace-nowrap rounded-full px-3.5 text-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-white/60 [&_svg]:size-3.5";

/** A chip at rest. */
export const CHIP = `${SHAPE} text-slate-400 hover:bg-white/10 hover:text-white data-[state=open]:bg-white/10 data-[state=open]:text-white`;
/** A chip whose action is in effect ("Filters active"). */
export const CHIP_ON = `${SHAPE} bg-white/15 font-semibold text-white`;

type TabStripProps = { children: ReactNode; label: string };

export function TabStrip({ children, label }: TabStripProps) {
  return (
    <div
      role="toolbar"
      aria-label={label}
      className="flex w-fit max-w-full items-center gap-0.5 overflow-x-auto rounded-full border border-white/10 bg-surface/90 p-1 shadow-xl backdrop-blur-md"
    >
      {children}
    </div>
  );
}

export const TabStripDivider = () => <span aria-hidden className="mx-1 h-5 w-px shrink-0 bg-white/10" />;
