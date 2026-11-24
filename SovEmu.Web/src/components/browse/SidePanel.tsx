// A panel on the right edge over the dimmed page, full height, sliding in while `open`: the game
// panel over a system's grid, and Home's Continue panel. The page's pad navigation handles Back.

import { Dialog } from "radix-ui";
import type { ReactNode } from "react";

/** Stops the wheel and touch from scrolling the page through the dimming. React's own wheel
 *  and touch listeners are passive, so they could not cancel it. */
function blockScroll(el: HTMLDivElement | null) {
  if (!el) return;
  const stop = (e: Event) => e.preventDefault();
  el.addEventListener("wheel", stop, { passive: false });
  el.addEventListener("touchmove", stop, { passive: false });
  return () => {
    el.removeEventListener("wheel", stop);
    el.removeEventListener("touchmove", stop);
  };
}

export function SidePanel({
  open,
  onClose,
  keepOpenOn,
  children,
}: {
  open: boolean;
  onClose: () => void;
  /** A selector for what the panel stays open over when the focus moves to it (the grid's
   *  cards: moving to another switches the panel rather than closing it). */
  keepOpenOn?: string;
  children: ReactNode;
}) {
  return (
    <Dialog.Root open={open} modal={false} onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        {/* Dims the page, side nav included. A non-modal dialog has no Overlay, so this stands
            in for one: it takes the pointer, so nothing under it can be clicked or scrolled, and
            a click on it only closes the panel (Radix sees it as outside the panel). It keeps the
            pointer while it fades, so that click cannot land on a card under it. */}
        <div
          aria-hidden
          ref={blockScroll}
          data-state={open ? "open" : "closed"}
          className="fixed inset-0 z-[41] bg-black/60 motion-safe:data-[state=open]:animate-fade-in motion-safe:data-[state=closed]:animate-fade-out"
        />
        <Dialog.Content
          aria-describedby={undefined}
          // The page's pad navigation handles Escape (its Back).
          onEscapeKeyDown={(e) => e.preventDefault()}
          onInteractOutside={(e) => {
            const target = e.target as Element | null;
            const keep = ["[data-radix-popper-content-wrapper]", "nav", keepOpenOn].filter(Boolean).join(", ");
            if (target?.closest?.(keep)) e.preventDefault();
          }}
          onOpenAutoFocus={(e) => e.preventDefault()}
          className="fixed inset-y-0 right-0 z-[42] flex w-[min(28rem,100vw)] flex-col overflow-hidden border-l border-white/10 bg-surface/85 shadow-2xl outline-none backdrop-blur-xl backdrop-saturate-150 motion-safe:data-[state=open]:animate-slide-in-right motion-safe:data-[state=closed]:animate-slide-out-right"
        >
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
