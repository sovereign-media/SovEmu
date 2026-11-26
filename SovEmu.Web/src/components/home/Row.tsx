// A home screen row: a titled, horizontally scrolling strip of game or system cards.
import { ChevronLeft, ChevronRight, Play } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import type { Game, RecentGame, SystemSummary } from "../../lib/api";
import { cardArt, nextScroll } from "../../lib/home";
import { systemBadge } from "../../lib/systems";
import { agoFromSqlite } from "../../lib/time";
import { cn } from "../ui/cn";

type RowProps = { title: string; icon: ReactNode; children: ReactNode };

export function Row({ title, icon, children }: RowProps) {
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

type GameCardProps = {
  game: Game | RecentGame;
  row: number;
  col: number;
  /** Landscape (where the player left off) rather than box-art portrait. */
  wide?: boolean;
  focused: boolean;
  color: string;
  short: string;
  onOpen: () => void;
};

export function GameCard({ game, row, col, wide, focused, color, short, onOpen }: GameCardProps) {
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
            className={cn(
              "absolute inset-0 size-full",
              art.kind === "screenshot" ? "pixelated object-cover" : wide ? "object-cover" : "object-contain",
            )}
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
          {game.release && <span className="font-medium text-amber-200/90"> · {game.release}</span>}
          {game.lastPlayedAt && wide ? ` · ${agoFromSqlite(game.lastPlayedAt)}` : ""}
        </p>
      </div>
    </button>
  );
}

/** The short name is the card's graphic: as large as leaves it readable beside the photo. */
const shortNameSize = (short: string) => (short.length <= 2 ? 78 : short.length === 3 ? 68 : 54);

type SystemCardProps = {
  system: SystemSummary;
  row: number;
  col: number;
  focused: boolean;
  onOpen: () => void;
};

export function SystemCard({ system: s, row, col, focused, onOpen }: SystemCardProps) {
  const badge = systemBadge(s);
  const photo = s.photoUrl;
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
          "relative h-32 overflow-hidden rounded-2xl border border-edge bg-linear-160 from-[#1b2130] to-[#10131b] shadow-xl transition-all duration-200",
          ring(focused),
        )}
      >
        <span
          className="absolute top-1/2 left-2.5 -translate-y-1/2 leading-none font-black tracking-[-0.06em]"
          style={{ color: `color-mix(in srgb, ${s.color} 68%, white)`, fontSize: shortNameSize(s.short) }}
        >
          {s.short}
        </span>
        {photo && (
          <img
            src={photo}
            alt=""
            decoding="async"
            className="absolute right-2 bottom-2 h-[76%] w-[58%] object-contain object-right-bottom drop-shadow-[-6px_8px_10px_rgb(0_0_0/0.6)]"
          />
        )}
        <span className="absolute inset-x-0 bottom-0 h-[3px]" style={{ background: s.color }} />
        {badge && (
          <span className="absolute top-2 right-2 rounded bg-black/70 px-2 py-0.5 text-[11px] font-semibold text-white">
            {badge}
          </span>
        )}
      </div>
      <div className="min-w-0 px-0.5">
        <p className="truncate text-sm font-semibold">{s.name}</p>
        <p className="text-xs text-slate-400">
          {s.manufacturer} · {s.year} · {s.gameCount} game{s.gameCount === 1 ? "" : "s"}
        </p>
      </div>
    </button>
  );
}
