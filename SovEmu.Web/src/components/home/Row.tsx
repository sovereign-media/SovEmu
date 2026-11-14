// A home screen row: a titled, horizontally scrolling strip of game or system cards.
import { ChevronLeft, ChevronRight, Play } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { formatAgo, type Game, type RecentGame, type SystemSummary } from "../../lib/api";
import { cardArt, nextScroll } from "../../lib/home";
import { systemBadge } from "../../lib/systems";
import { cn } from "../ui/cn";

export function Row({ title, icon, children }: { title: string; icon: ReactNode; children: ReactNode }) {
  const strip = useRef<HTMLDivElement>(null);
  // Arrows only when the row is wider than the screen; measured on resize.
  const [overflows, setOverflows] = useState(false);
  useEffect(() => {
    const v = strip.current;
    if (!v) return;
    const measure = () => setOverflows(v.scrollWidth > v.clientWidth + 4);
    measure();
    const resize = new ResizeObserver(measure);
    resize.observe(v);
    return () => resize.disconnect();
  }, []);
  const scroll = (dir: -1 | 1) => {
    const v = strip.current;
    if (v) v.scrollTo({ left: nextScroll(v.scrollLeft, v.clientWidth, v.scrollWidth, dir), behavior: "smooth" });
  };
  const arrow = "rounded-lg p-1.5 text-slate-400 transition-colors hover:bg-white/10 hover:text-white";
  return (
    <section className="min-w-0">
      <div className="mb-3 flex items-center justify-between px-8">
        <h2 className="flex items-center gap-2 text-lg font-bold [&_svg]:size-5 [&_svg]:text-slate-400">
          {icon}
          {title}
        </h2>
        {overflows && (
          <div className="flex gap-1">
            <button type="button" aria-label={`Scroll ${title} left`} onClick={() => scroll(-1)} className={arrow}>
              <ChevronLeft className="size-5" />
            </button>
            <button type="button" aria-label={`Scroll ${title} right`} onClick={() => scroll(1)} className={arrow}>
              <ChevronRight className="size-5" />
            </button>
          </div>
        )}
      </div>
      <div ref={strip} data-row-strip className="no-scrollbar overflow-x-auto overscroll-x-contain">
        <div className="flex w-max gap-4 px-8 py-3">{children}</div>
      </div>
    </section>
  );
}

const ring = (focused: boolean) => (focused ? "scale-105 ring-4 ring-white/80" : "hover:scale-[1.03]");

export function GameCard({
  game,
  row,
  col,
  wide,
  focused,
  color,
  short,
  onOpen,
}: {
  game: Game | RecentGame;
  row: number;
  col: number;
  /** Landscape (where the player left off) rather than box-art portrait. */
  wide?: boolean;
  focused: boolean;
  color: string;
  short: string;
  onOpen: () => void;
}) {
  const art = cardArt(game);
  return (
    <button
      type="button"
      data-row={row}
      data-col={col}
      onClick={onOpen}
      className={cn("group grid shrink-0 cursor-pointer gap-2 text-left outline-none", wide ? "w-72" : "w-40")}
    >
      <div
        className={cn(
          "relative grid place-items-center overflow-hidden rounded-xl border border-white/10 shadow-xl transition-all duration-200",
          wide ? "aspect-video" : "aspect-[3/4]",
          ring(focused),
        )}
        style={{ background: `linear-gradient(145deg, ${color}, color-mix(in srgb, ${color} 30%, black))` }}
      >
        <span className="text-3xl font-black text-white/80">{short}</span>
        {art && (
          <img
            src={art.url}
            alt=""
            loading="lazy"
            className={cn("absolute inset-0 size-full", art.kind === "screenshot" ? "pixelated object-cover" : wide ? "object-cover" : "object-contain")}
          />
        )}
        {wide && (
          <span className="absolute inset-0 grid place-items-center bg-black/40 opacity-0 transition-opacity group-hover:opacity-100">
            <Play className="size-10 fill-white text-white" />
          </span>
        )}
      </div>
      <div className="min-w-0 px-0.5">
        <p className="truncate text-sm font-semibold">{game.title}</p>
        <p className="truncate text-xs text-slate-400">
          {short}
          {game.last_played_at && wide ? ` · ${formatAgo(game.last_played_at)}` : ""}
        </p>
      </div>
    </button>
  );
}

export function SystemCard({
  system: s,
  row,
  col,
  focused,
  onOpen,
}: {
  system: SystemSummary;
  row: number;
  col: number;
  focused: boolean;
  onOpen: () => void;
}) {
  const badge = systemBadge(s);
  return (
    <button
      type="button"
      data-row={row}
      data-col={col}
      onClick={onOpen}
      className="grid w-56 shrink-0 cursor-pointer gap-2 text-left outline-none"
    >
      <div
        className={cn(
          "relative grid h-32 place-items-center rounded-2xl border border-white/10 text-4xl font-black tracking-tight text-white shadow-xl transition-all duration-200",
          ring(focused),
        )}
        style={{ background: `linear-gradient(145deg, ${s.color}, color-mix(in srgb, ${s.color} 35%, black))` }}
      >
        {s.short}
        {badge && (
          <span className="absolute right-3 bottom-2 rounded bg-black/60 px-2 py-0.5 text-[11px] font-semibold tracking-normal">{badge}</span>
        )}
      </div>
      <div className="min-w-0 px-0.5">
        <p className="truncate text-sm font-semibold">{s.name}</p>
        <p className="text-xs text-slate-400">
          {s.manufacturer} · {s.year} · {s.game_count} game{s.game_count === 1 ? "" : "s"}
        </p>
      </div>
    </button>
  );
}
