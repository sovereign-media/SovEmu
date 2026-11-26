// Jump to a letter: Sovereign's carousel of letter discs, the current one large in the middle.
// Left and right (pad or keys, handled by the page) move along it and the grid follows after a
// short rest; A or Enter goes there, B or Escape goes back to where the grid was.
import { BookA, ChevronLeft, ChevronRight } from "lucide-react";
import { Dialog } from "radix-ui";
import { stripWindow } from "../../lib/games";
import { cn } from "../ui/cn";
import { Tooltip } from "../ui/primitives";
import { CHIP } from "./TabStrip";

type JumpButtonProps = { onClick: () => void; disabled?: boolean };

/** Opens the jump strip. Letters only mean something in title order: with another sort the button
 *  stays where it is, disabled, and says why (`aria-disabled`, so its tooltip still shows). */
export function JumpButton({ onClick, disabled }: JumpButtonProps) {
  const button = (
    <button
      type="button"
      onClick={disabled ? undefined : onClick}
      aria-haspopup="dialog"
      aria-disabled={disabled || undefined}
      className={cn(CHIP, disabled && "cursor-default opacity-40 hover:bg-transparent hover:text-slate-400")}
    >
      <BookA /> Jump
    </button>
  );
  return disabled ? <Tooltip content="Sort by title to jump to a letter">{button}</Tooltip> : button;
}

// The outermost neighbours drop out on a phone, where seven discs do not fit.
const DISTANCE = ["", "flex", "flex", "hidden sm:flex"];

type DiscProps = { stop: string; distance: number; onClick: () => void };

function Disc({ stop, distance, onClick }: DiscProps) {
  return (
    <button
      type="button"
      tabIndex={-1}
      onClick={onClick}
      aria-label={`Go to ${stop}`}
      className={cn(
        DISTANCE[distance],
        "size-11 shrink-0 cursor-pointer items-center justify-center rounded-full text-lg font-bold text-white hover:bg-white/10",
      )}
    >
      {stop}
    </button>
  );
}

type JumpStripProps = {
  stops: string[];
  index: number;
  onIndex: (i: number) => void;
  onCommit: () => void;
  onCancel: () => void;
};

export function JumpStrip({ stops, index, onIndex, onCommit, onCancel }: JumpStripProps) {
  const { before, current, after } = stripWindow(stops, index);
  const arrow = "grid size-8 shrink-0 cursor-pointer place-items-center rounded-full text-slate-400 hover:text-white";
  return (
    <Dialog.Root open onOpenChange={(o) => !o && onCancel()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-surface/50" />
        <Dialog.Content
          // The page's pad navigation handles Escape (its Back), so it is not handled twice.
          onEscapeKeyDown={(e) => e.preventDefault()}
          className="fixed top-1/2 left-1/2 z-50 flex max-w-[calc(100vw-1.5rem)] -translate-x-1/2 -translate-y-1/2 items-center gap-3 rounded-full border border-white/10 bg-surface/95 px-4 py-3 shadow-2xl outline-none backdrop-blur-md"
        >
          <Dialog.Title className="sr-only">Jump to letter</Dialog.Title>
          <Dialog.Description className="sr-only">Left and right choose a letter; Enter goes there.</Dialog.Description>
          <button
            type="button"
            aria-label="Previous letter"
            onClick={() => onIndex(Math.max(0, index - 1))}
            className={cn(arrow, !before.length && "invisible")}
          >
            <ChevronLeft className="size-5" />
          </button>
          <div className="flex w-[9.5rem] justify-end gap-1 sm:w-[14.5rem]">
            {before.map((stop, i) => (
              <Disc key={stop} stop={stop} distance={before.length - i} onClick={() => onIndex(stops.indexOf(stop))} />
            ))}
          </div>
          <button
            type="button"
            onClick={onCommit}
            aria-label={`Go to ${current}`}
            className="grid size-16 shrink-0 cursor-pointer place-items-center rounded-full bg-accent/20 text-3xl font-bold text-white ring-2 ring-accent"
          >
            {current}
          </button>
          <div className="flex w-[9.5rem] gap-1 sm:w-[14.5rem]">
            {after.map((stop, i) => (
              <Disc key={stop} stop={stop} distance={i + 1} onClick={() => onIndex(stops.indexOf(stop))} />
            ))}
          </div>
          <button
            type="button"
            aria-label="Next letter"
            onClick={() => onIndex(Math.min(stops.length - 1, index + 1))}
            className={cn(arrow, !after.length && "invisible")}
          >
            <ChevronRight className="size-5" />
          </button>
          <span aria-live="polite" className="sr-only">
            {current}
          </span>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
