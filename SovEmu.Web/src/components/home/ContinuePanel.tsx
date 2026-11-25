// Home's panel for a game in "Continue playing": where the player left off (the auto save's
// screenshot), when, and Continue; Game details goes on to the game's own panel on its system's
// page (Start over, the other saves, the description). The Android and iOS apps show the same.

import { History, Info, Play, Save, X } from "lucide-react";
import { Dialog } from "radix-ui";
import { type ReactNode, useEffect, useState } from "react";
import type { RecentGame } from "../../lib/api";
import type { ResumeButton } from "../../lib/home";
import { ago, agoFromSqlite } from "../../lib/time";
import { SidePanel } from "../browse/SidePanel";
import { Button } from "../ui/button";
import { cn } from "../ui/cn";

function Fact({ icon, children }: { icon: ReactNode; children: ReactNode }) {
  return (
    <span className="flex items-center gap-1.5 text-sm text-slate-300 [&_svg]:size-4 [&_svg]:text-slate-500">
      {icon}
      {children}
    </span>
  );
}

/** The auto save's screenshot, whole and on black like a TV; the game's artwork if there is none. */
function LeftOff({ game, color, short }: { game: RecentGame; color: string; short: string }) {
  const shot = game.autoSave?.screenshotUrl;
  if (shot) {
    return (
      <div className="grid aspect-video shrink-0 place-items-center bg-black">
        <img src={shot} alt="" className="pixelated size-full object-contain" />
      </div>
    );
  }
  const still = game.image ?? game.cover ?? game.thumb;
  return (
    <div
      className="relative grid aspect-video shrink-0 place-items-center overflow-hidden text-6xl font-black text-white/80"
      style={{ background: `linear-gradient(145deg, ${color}, color-mix(in srgb, ${color} 30%, black))` }}
    >
      <span>{short}</span>
      {still && (
        <img
          src={still}
          alt=""
          className={cn("absolute inset-0 size-full", game.image ? "object-cover" : "object-contain")}
        />
      )}
    </div>
  );
}

export function ContinuePanel({
  game,
  colorOf,
  shortOf,
  focused,
  onClose,
  onPlay,
  onDetails,
}: {
  game: RecentGame | null;
  /** A system's colour and short name, by its id. */
  colorOf: (system: string) => string;
  shortOf: (system: string) => string;
  /** The button the pad or keyboard is on, ringed; null while the mouse is in use. */
  focused: ResumeButton | null;
  onClose: () => void;
  /** Continue from the auto save, or Play from the start when there is none. */
  onPlay: (start: "continue" | "new") => void;
  onDetails: () => void;
}) {
  // The last game stays drawn while the panel slides out after closing.
  const [shown, setShown] = useState(game);
  useEffect(() => {
    if (game) setShown(game);
  }, [game]);
  return (
    <SidePanel open={!!game} onClose={onClose}>
      {shown && (
        <>
          <div className="relative">
            <LeftOff game={shown} color={colorOf(shown.system)} short={shortOf(shown.system)} />
            <Dialog.Close
              aria-label="Close"
              className="absolute top-3 right-3 grid size-9 place-items-center rounded-full bg-black/50 text-white backdrop-blur hover:bg-black/70"
            >
              <X className="size-5" />
            </Dialog.Close>
          </div>
          <div className="grid min-h-0 flex-1 content-start gap-4 overflow-y-auto px-6 pt-4 pb-6">
            <div>
              <span
                className="inline-block rounded px-2 py-0.5 text-xs font-bold text-white"
                style={{ background: colorOf(shown.system) }}
              >
                {shortOf(shown.system)}
              </span>
              <Dialog.Title className="mt-2.5 text-2xl leading-tight font-black">{shown.title}</Dialog.Title>
              {shown.release && <p className="mt-1 text-sm font-medium text-amber-200/90">{shown.release}</p>}
            </div>
            <div className="grid gap-1.5">
              {shown.autoSave && <Fact icon={<Save />}>Saved {ago(shown.autoSave.savedAt)}</Fact>}
              {shown.playCount > 0 && (
                <Fact icon={<History />}>
                  Played {shown.playCount}× · last {agoFromSqlite(shown.lastPlayedAt)}
                </Fact>
              )}
            </div>
            <div className="flex flex-wrap gap-2">
              {/* A game played without an auto save (it never got one) has nothing to continue. */}
              <Button
                size="lg"
                className={cn(focused === "play" && "ring-4 ring-white/60")}
                onClick={() => onPlay(shown.autoSave ? "continue" : "new")}
              >
                <Play className="fill-current" /> {shown.autoSave ? "Continue" : "Play"}
              </Button>
              <Button
                size="lg"
                variant="outline"
                className={cn(focused === "details" && "ring-4 ring-white/60")}
                onClick={onDetails}
              >
                <Info /> Game details
              </Button>
            </div>
          </div>
        </>
      )}
    </SidePanel>
  );
}
