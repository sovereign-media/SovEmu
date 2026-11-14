// A game's details in a side panel over the grid: artwork (and its video snap), facts,
// description, Continue / Start over or Play, save slots and play history. A translucent overlay:
// the grid shows through it and never reflows for it. Not modal: the grid stays usable, and
// choosing another card switches the panel to it.
import { Building2, CalendarDays, Play, RotateCcw, Tag, Users, X } from "lucide-react";
import { Dialog } from "radix-ui";
import { useEffect, useState, type ReactNode } from "react";
import { AUTO_SLOT, formatAgo, formatSize, type Game, type SaveSlot, type SystemSummary } from "../../lib/api";
import { maxPlayers } from "../../lib/games";
import { Button } from "../ui/button";
import { cn } from "../ui/cn";
import { Tooltip } from "../ui/primitives";

function Art({ game, color, short }: { game: Game; color: string; short: string }) {
  const [video, setVideo] = useState(false);
  const [playing, setPlaying] = useState(false);
  useEffect(() => {
    setVideo(false);
    setPlaying(false);
    if (!game.video) return;
    const t = setTimeout(() => setVideo(true), 900);
    return () => clearTimeout(t);
  }, [game.id]);
  const still = game.image ?? game.thumb;
  return (
    <div
      className="relative grid aspect-video shrink-0 place-items-center overflow-hidden text-6xl font-black text-white/80"
      style={{ background: `linear-gradient(145deg, ${color}, color-mix(in srgb, ${color} 30%, black))` }}
    >
      <span>{short}</span>
      {still && (
        <img key={game.id} src={still} alt="" className={cn("absolute inset-0 size-full", game.image ? "object-cover" : "object-contain")} />
      )}
      {video && game.video && (
        <video
          src={game.video}
          autoPlay
          muted
          loop
          playsInline
          onPlaying={() => setPlaying(true)}
          className={cn("absolute inset-0 size-full object-cover", !playing && "hidden")}
        />
      )}
      <div className="absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-surface/80 to-transparent" />
      {game.image && game.thumb && (
        <img src={game.thumb} alt="" className="absolute bottom-3 left-5 h-1/2 rounded-md object-contain shadow-2xl" />
      )}
    </div>
  );
}

function Fact({ icon, children }: { icon: ReactNode; children: ReactNode }) {
  return (
    <span className="flex items-center gap-1.5 text-sm text-slate-300 [&_svg]:size-4 [&_svg]:text-slate-500">
      {icon}
      {children}
    </span>
  );
}

const ago = (unix: number) => formatAgo(new Date(unix * 1000).toISOString().replace("T", " ").slice(0, 19));

export function GamePanel({
  game,
  system,
  slots,
  onClose,
  onPlay,
}: {
  game: Game | null;
  system: SystemSummary;
  slots: SaveSlot[];
  onClose: () => void;
  onPlay: (start: "continue" | "new") => void;
}) {
  const auto = slots.find((s) => s.slot === AUTO_SLOT);
  const players = maxPlayers(game?.players ?? null);
  const year = game?.releaseDate?.slice(0, 4);
  return (
    <Dialog.Root open={!!game} modal={false} onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Content
          aria-describedby={undefined}
          // The page's pad navigation handles Escape (its Back); a click on another card switches
          // the panel instead of closing it.
          onEscapeKeyDown={(e) => e.preventDefault()}
          onInteractOutside={(e) => {
            const target = e.target as Element | null;
            if (target?.closest?.("[data-game-card], [data-radix-popper-content-wrapper], nav")) e.preventDefault();
          }}
          onOpenAutoFocus={(e) => e.preventDefault()}
          className="fixed inset-y-3 right-3 z-30 flex w-[min(28rem,calc(100vw-1.5rem))] flex-col overflow-hidden rounded-[2rem] border border-white/15 bg-surface/60 shadow-2xl outline-none backdrop-blur-xl backdrop-saturate-150"
        >
          {game && (
            <>
              <div className="relative">
                <Art game={game} color={system.color} short={system.short} />
                <Dialog.Close
                  aria-label="Close"
                  className="absolute top-3 right-3 grid size-9 place-items-center rounded-full bg-black/50 text-white backdrop-blur hover:bg-black/70"
                >
                  <X className="size-5" />
                </Dialog.Close>
              </div>
              <div className="grid min-h-0 flex-1 content-start gap-4 overflow-y-auto px-6 pt-2 pb-6">
                <div>
                  <Dialog.Title className="text-2xl leading-tight font-black">{game.title}</Dialog.Title>
                  {game.tags.length > 0 && <p className="mt-1 text-xs text-slate-500">{game.tags.join(" · ")}</p>}
                  <div className="mt-3 flex flex-wrap gap-x-4 gap-y-1">
                    {year && <Fact icon={<CalendarDays />}>{year}</Fact>}
                    {(game.developer || game.publisher) && <Fact icon={<Building2 />}>{game.developer ?? game.publisher}</Fact>}
                    {game.genre && <Fact icon={<Tag />}>{game.genre}</Fact>}
                    {players !== null && <Fact icon={<Users />}>{players === 1 ? "1 player" : `${game.players} players`}</Fact>}
                  </div>
                </div>
                <div className="flex flex-wrap gap-2">
                  {auto ? (
                    <>
                      <Button size="lg" onClick={() => onPlay("continue")}>
                        <Play className="fill-current" /> Continue
                      </Button>
                      <Button size="lg" variant="outline" onClick={() => onPlay("new")}>
                        <RotateCcw /> Start over
                      </Button>
                    </>
                  ) : (
                    <Button size="lg" onClick={() => onPlay("new")}>
                      <Play className="fill-current" /> Play
                    </Button>
                  )}
                </div>
                {game.description && <p className="text-sm leading-relaxed text-slate-300">{game.description}</p>}
                {slots.length > 0 && (
                  <div>
                    <p className="mb-2 text-xs font-semibold tracking-wider text-slate-500 uppercase">Saves</p>
                    <div className="flex flex-wrap gap-2">
                      {slots.map((s) => (
                        <Tooltip key={s.slot} content={`${s.slot === AUTO_SLOT ? "Auto save" : `Slot ${s.slot}`} · ${ago(s.savedAt)}`}>
                          <span className="pixelated grid h-12 w-16 place-items-center overflow-hidden rounded border border-white/10 bg-black/50 text-[10px] text-slate-400">
                            {s.screenshotUrl ? (
                              <img src={s.screenshotUrl} alt="" className="pixelated size-full object-cover" />
                            ) : s.slot === AUTO_SLOT ? (
                              "Auto"
                            ) : (
                              `Slot ${s.slot}`
                            )}
                          </span>
                        </Tooltip>
                      ))}
                    </div>
                  </div>
                )}
                <p className="border-t border-white/10 pt-3 text-xs text-slate-500">
                  {game.playCount ? `Played ${game.playCount}× · last ${formatAgo(game.lastPlayedAt)}` : "Not played yet"}
                  {" · "}
                  <span className="font-mono break-all">{game.path}</span> · {formatSize(game.size)}
                </p>
              </div>
            </>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
