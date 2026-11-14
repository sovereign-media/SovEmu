// The home screen's hero: featured games, one at a time, over their artwork. Arrows, dots,
// a swipe or the pad's left/right change game; it moves on by itself every few seconds unless
// the pointer is over it.
import { ChevronLeft, ChevronRight, Gamepad2, LayoutGrid, Play } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Game } from "../../lib/api";
import { byline, heroArt } from "../../lib/home";
import { Button } from "../ui/button";
import { cn } from "../ui/cn";

const ADVANCE_MS = 9000;

export function Hero({
  games,
  index,
  onIndex,
  focused,
  color,
  short,
  onPlay,
  onBrowse,
  empty,
}: {
  games: Game[];
  index: number;
  onIndex: (i: number) => void;
  focused: boolean;
  color: (system: string) => string;
  short: (system: string) => string;
  onPlay: (g: Game) => void;
  onBrowse: (g: Game) => void;
  /** No games at all: say how to add some. */
  empty: boolean;
}) {
  const [hovered, setHovered] = useState(false);
  const touch = useRef<number | null>(null);
  const n = games.length;
  const go = (i: number) => n && onIndex((i + n) % n);

  // Auto-advance; any change of slide restarts the clock.
  useEffect(() => {
    if (n < 2 || hovered) return;
    const t = setTimeout(() => onIndex((index + 1) % n), ADVANCE_MS);
    return () => clearTimeout(t);
  }, [index, n, hovered, onIndex]);

  const box = "relative w-full overflow-hidden bg-black aspect-square min-h-80 sm:aspect-[16/9] lg:aspect-[16/6.3]";
  if (!n) {
    return (
      <div className={cn(box, "grid place-items-center bg-[radial-gradient(ellipse_at_top,#1e2a4a,transparent_70%)]")}>
        <div className="grid justify-items-center gap-3 px-8 text-center text-slate-400">
          <Gamepad2 className="size-12 text-slate-600" />
          {empty ? (
            <p>
              No games yet. Add ROMs to the library (one folder per system: <code>nes/</code>, <code>snes/</code>, …) and rescan.
            </p>
          ) : (
            <p>No artwork yet: fetch game information on the Library page to fill this space.</p>
          )}
        </div>
      </div>
    );
  }

  const game = games[Math.min(index, n - 1)];
  const line = byline(game);
  return (
    <section
      aria-roledescription="carousel"
      aria-label="Featured games"
      className={box}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onTouchStart={(e) => (touch.current = e.touches[0].clientX)}
      onTouchEnd={(e) => {
        if (touch.current === null) return;
        const dx = e.changedTouches[0].clientX - touch.current;
        touch.current = null;
        if (Math.abs(dx) > 40) go(index + (dx < 0 ? 1 : -1));
      }}
    >
      {/* Every slide stays mounted, so changing game cross-fades instead of blanking. */}
      {games.map((g, i) => {
        const art = heroArt(g);
        const boxArtOnly = !g.media.includes("image");
        const c = color(g.system);
        return (
          <div
            key={g.id}
            aria-hidden={i !== index}
            className={cn("absolute inset-0 transition-opacity duration-700", i === index ? "opacity-100" : "opacity-0")}
            style={{ background: `linear-gradient(135deg, ${c}, color-mix(in srgb, ${c} 25%, black))` }}
          >
            {art && (
              <img
                src={art}
                alt=""
                loading={i === index ? "eager" : "lazy"}
                className={cn("size-full object-cover", boxArtOnly && "scale-110 opacity-50 blur-2xl")}
              />
            )}
            {art && boxArtOnly && (
              <img
                src={art}
                alt=""
                className="absolute top-1/2 right-[8%] hidden h-[70%] -translate-y-1/2 rounded-lg object-contain shadow-2xl lg:block"
              />
            )}
          </div>
        );
      })}

      <div className="absolute inset-0 bg-gradient-to-r from-black/85 via-black/35 to-transparent" />
      <div className="absolute inset-0 bg-gradient-to-t from-black via-black/30 to-transparent" />
      <div className="absolute inset-x-0 bottom-0 h-24 bg-gradient-to-t from-surface to-transparent" />

      <div className="absolute bottom-0 left-0 max-w-2xl p-6 pr-12 pb-14 sm:p-12 sm:pb-16">
        <span className="rounded-md px-2 py-0.5 text-xs font-black text-white" style={{ background: color(game.system) }}>
          {short(game.system)}
        </span>
        <h2 className="mt-3 text-3xl leading-tight font-black text-white sm:text-5xl">{game.title}</h2>
        {line && <p className="mt-2 text-sm text-slate-300">{line}</p>}
        {game.description && (
          <p className="mt-3 line-clamp-2 text-sm leading-relaxed text-slate-300 sm:line-clamp-3">{game.description}</p>
        )}
        <div className="mt-5 flex gap-2">
          <Button size="lg" variant="light" onClick={() => onPlay(game)} className={cn(focused && "ring-4 ring-white/60")}>
            <Play className="fill-current" /> Play
          </Button>
          <Button size="lg" variant="glass" onClick={() => onBrowse(game)}>
            <LayoutGrid /> More {short(game.system)} games
          </Button>
        </div>
      </div>

      {n > 1 && (
        <>
          <button
            type="button"
            onClick={() => go(index - 1)}
            aria-label="Previous game"
            className="absolute top-1/2 left-3 hidden -translate-y-1/2 cursor-pointer rounded-full bg-black/40 p-2 text-white backdrop-blur-sm transition-colors hover:bg-black/70 sm:block"
          >
            <ChevronLeft className="size-6" />
          </button>
          <button
            type="button"
            onClick={() => go(index + 1)}
            aria-label="Next game"
            className="absolute top-1/2 right-3 hidden -translate-y-1/2 cursor-pointer rounded-full bg-black/40 p-2 text-white backdrop-blur-sm transition-colors hover:bg-black/70 sm:block"
          >
            <ChevronRight className="size-6" />
          </button>
          <div className="absolute right-4 bottom-4 flex sm:right-6 sm:bottom-6">
            {games.map((g, i) => (
              <button
                key={g.id}
                type="button"
                aria-label={`Show ${g.title}`}
                aria-current={i === index}
                onClick={() => go(i)}
                className="group flex h-6 cursor-pointer items-center px-[3px]"
              >
                <span
                  className={cn(
                    "block h-2 rounded-full transition-all",
                    i === index ? "w-5 bg-white" : "w-2 bg-white/40 group-hover:bg-white/70",
                  )}
                />
              </button>
            ))}
          </div>
        </>
      )}
    </section>
  );
}
