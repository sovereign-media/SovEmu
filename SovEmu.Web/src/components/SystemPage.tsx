// One system's games as a grid of covers, after Sovereign's browse pages: a header with a capsule
// of chips (Sort, Filters, Jump), the grid, and a side panel with the chosen game's details.
// The panel has its own URL (`/system/?id=nes&game=42`): opening one there opens the panel, Back
// closes it, and moving between games with it open replaces the URL rather than stacking it.
//
// Pad and keyboard: the d-pad moves through the grid, A opens the panel (and in it plays), B
// closes the panel or goes home, Start (or "/") opens the filters, the shoulders move three rows.
// The list logic is src/lib/games.ts.
import { ArrowDownAZ, Check, CircleCheck, SlidersHorizontal, Users } from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { useEffect, useMemo, useRef, useState } from "react";
import { api, AUTO_SLOT, type Game, type SaveSlot, type SystemSummary } from "../lib/api";
import {
  SORTS,
  activeFilters,
  filterGames,
  gameFromUrl,
  gridMove,
  jumpStops,
  jumpTo,
  letterOf,
  maxPlayers,
  sortGames,
  urlWithGame,
  type GameFilters,
  type GameSort,
} from "../lib/games";
import { listenNav } from "../lib/nav";
import { FilterDrawer } from "./browse/FilterDrawer";
import { GamePanel } from "./browse/GamePanel";
import { JumpButton, JumpStrip } from "./browse/JumpStrip";
import { CHIP, CHIP_ON, TabStrip, TabStripDivider } from "./browse/TabStrip";
import HintBar from "./HintBar";
import Shell from "./Shell";
import SideNav, { NAV_SPACE } from "./SideNav";
import { cn } from "./ui/cn";

const SORT_KEY = "sovemu.sort";
/** How long the jump strip rests on a letter before the grid follows it. */
const FOLLOW_DELAY_MS = 300;

/** A cover in the grid. */
function GameCard({
  game,
  system,
  focused,
  open,
  onOpen,
}: {
  game: Game;
  system: SystemSummary;
  focused: boolean;
  open: boolean;
  onOpen: () => void;
}) {
  const art = game.thumb ?? game.image;
  const players = maxPlayers(game.players);
  return (
    <button
      type="button"
      data-game-card={game.id}
      onClick={onOpen}
      aria-label={game.title}
      aria-current={open || undefined}
      className="group grid cursor-pointer content-start gap-2 text-left outline-none"
    >
      <div
        className={cn(
          "relative grid aspect-[3/4] place-items-center overflow-hidden rounded-xl border border-white/10 shadow-lg transition-all duration-150",
          focused
            ? "scale-105 ring-4 ring-white/80"
            : open
              ? "ring-2 ring-accent"
              : "group-hover:scale-[1.03] group-focus-visible:ring-2 group-focus-visible:ring-white/60",
        )}
        style={{ background: `linear-gradient(145deg, ${system.color}, color-mix(in srgb, ${system.color} 30%, black))` }}
      >
        <span className="px-3 text-center text-sm font-bold text-white/80">{game.title}</span>
        {art && <img src={art} alt="" loading="lazy" className="absolute inset-0 size-full object-cover" />}
        {!!game.playCount && (
          <span className="absolute top-2 right-2 grid size-6 place-items-center rounded-full bg-black/70 text-white" title="Played">
            <CircleCheck className="size-4" />
          </span>
        )}
      </div>
      <div className="min-w-0 px-0.5">
        <p className="truncate text-sm font-semibold">{game.title}</p>
        <p className="flex items-center gap-1.5 truncate text-xs text-slate-400">
          {game.releaseDate?.slice(0, 4) ?? game.tags[0] ?? system.short}
          {players !== null && players >= 2 && (
            <span className="flex items-center gap-0.5">
              · <Users className="size-3" /> {players}
            </span>
          )}
        </p>
      </div>
    </button>
  );
}

function SortChip({ sort, onSort }: { sort: GameSort; onSort: (s: GameSort) => void }) {
  const label = SORTS.find((s) => s.value === sort)?.label ?? "Title";
  return (
    <DropdownMenu.Root modal={false}>
      <DropdownMenu.Trigger className={CHIP} aria-label={`Sort: ${label}`}>
        <ArrowDownAZ /> {label}
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content sideOffset={8} align="start" className="z-50 min-w-44 rounded-xl border border-edge bg-panel p-1 shadow-2xl">
          <DropdownMenu.RadioGroup value={sort} onValueChange={(v) => onSort(v as GameSort)}>
            {SORTS.map((s) => (
              <DropdownMenu.RadioItem
                key={s.value}
                value={s.value}
                className="relative flex cursor-pointer items-center rounded-lg py-1.5 pr-3 pl-8 text-sm text-slate-200 outline-none select-none data-[highlighted]:bg-white/10"
              >
                <DropdownMenu.ItemIndicator className="absolute left-2.5">
                  <Check className="size-3.5" />
                </DropdownMenu.ItemIndicator>
                {s.label}
              </DropdownMenu.RadioItem>
            ))}
          </DropdownMenu.RadioGroup>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

function Games() {
  const systemId = new URLSearchParams(location.search).get("id") ?? "";
  const [system, setSystem] = useState<SystemSummary | null>(null);
  const [games, setGames] = useState<Game[]>([]);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<GameSort>(() => (localStorage.getItem(SORT_KEY) as GameSort) || "title");
  const [filters, setFilters] = useState<GameFilters>({});
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [focus, setFocus] = useState(0);
  // Focus rings show once a pad or the keyboard is used, not for the mouse.
  const [navigating, setNavigating] = useState(false);
  const [openId, setOpenId] = useState<number | null>(() => gameFromUrl(location.search));
  const [slots, setSlots] = useState<SaveSlot[]>([]);
  const [jump, setJump] = useState<{ index: number; scrollY: number } | null>(null);
  const grid = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);
  /** Whether this page pushed the panel's history entry (so closing it goes Back). */
  const pushed = useRef(false);

  useEffect(() => {
    api.system(systemId).then((s) => {
      if (!s) return location.replace("/");
      const { games, ...system } = s;
      document.title = `${system.name} · SovEmu`;
      setSystem(system);
      setGames(games);
    });
    const onPop = () => {
      pushed.current = false;
      setOpenId(gameFromUrl(location.search));
    };
    addEventListener("popstate", onPop);
    return () => removeEventListener("popstate", onPop);
  }, []);

  const shown = useMemo(() => sortGames(filterGames(games, query, filters), sort), [games, query, filters, sort]);
  const stops = useMemo(() => jumpStops(shown), [shown]);
  const open = openId === null ? null : (games.find((g) => g.id === openId) ?? null);

  // The panel's game in the grid, when it is shown: the focus starts there.
  useEffect(() => {
    if (openId === null) return;
    const i = shown.findIndex((g) => g.id === openId);
    if (i >= 0) setFocus(i);
  }, [openId, shown]);

  // Its saves, after a pause (moving quickly through games does not ask for each).
  useEffect(() => {
    setSlots([]);
    if (openId === null) return;
    const t = setTimeout(() => api.saves(openId).then(setSlots).catch(() => {}), 150);
    return () => clearTimeout(t);
  }, [openId]);

  /** Opens the panel on a game: a new history entry, or the same one when it is already open. */
  const openGame = (id: number) => {
    const url = urlWithGame(location.href, id);
    if (gameFromUrl(location.search) === null) {
      history.pushState(null, "", url);
      pushed.current = true;
    } else {
      history.replaceState(null, "", url);
    }
    setOpenId(id);
  };
  const closePanel = () => {
    if (gameFromUrl(location.search) === null) return setOpenId(null);
    if (pushed.current) {
      history.back(); // popstate closes it
    } else {
      history.replaceState(null, "", urlWithGame(location.href, null));
      setOpenId(null);
    }
  };
  const play = (g: Game | null, start: "continue" | "new") => {
    if (g) location.href = `/play/?game=${g.id}&start=${start}`;
  };
  const changeSort = (s: GameSort) => {
    setSort(s);
    localStorage.setItem(SORT_KEY, s);
  };

  const cardAt = (i: number) => grid.current?.querySelector(`[data-game-card="${latest.current.shown[i]?.id}"]`);

  // Keep the focused card in view.
  useEffect(() => {
    if (navigating) cardAt(focus)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [focus, navigating]);

  // The jump strip: the grid follows the letter it rests on.
  useEffect(() => {
    if (!jump) return;
    const t = setTimeout(() => {
      cardAt(jumpTo(shown, stops[jump.index]))?.scrollIntoView({ block: "center", behavior: "smooth" });
    }, FOLLOW_DELAY_MS);
    return () => clearTimeout(t);
  }, [jump?.index]);
  const openJump = () => {
    const at = shown[focus] ? stops.indexOf(letterOf(shown[focus].title)) : 0;
    setJump({ index: Math.max(0, at), scrollY });
  };
  const commitJump = () => {
    const { jump, shown, stops } = latest.current;
    if (!jump) return;
    setJump(null);
    setNavigating(true);
    setFocus(jumpTo(shown, stops[jump.index]));
  };
  const cancelJump = () => {
    const { jump } = latest.current;
    if (!jump) return;
    scrollTo({ top: jump.scrollY });
    setJump(null);
  };

  // Pad and keyboard, through the latest state.
  const latest = useRef({ shown, focus, open, slots, jump, filtersOpen, stops });
  latest.current = { shown, focus, open, slots, jump, filtersOpen, stops };
  const handlers = useRef({ openGame, closePanel, commitJump, cancelJump });
  handlers.current = { openGame, closePanel, commitJump, cancelJump };
  useEffect(() => {
    const nav = listenNav((action) => {
      const { shown, focus, open, slots, jump, filtersOpen, stops } = latest.current;
      const h = handlers.current;
      if (jump) {
        if (action === "left" || action === "right") {
          const index = Math.min(stops.length - 1, Math.max(0, jump.index + (action === "left" ? -1 : 1)));
          setJump({ ...jump, index });
        }
        if (action === "accept") h.commitJump();
        if (action === "back") h.cancelJump();
        return;
      }
      if (filtersOpen) {
        if (action === "back" || action === "menu") setFiltersOpen(false);
        return;
      }
      if (action === "menu") return setFiltersOpen(true);
      if (action === "back") return open ? h.closePanel() : void (location.href = "/");
      if (action === "accept") {
        if (open) return play(open, slots.some((s) => s.slot === AUTO_SLOT) ? "continue" : "new");
        if (shown[focus]) h.openGame(shown[focus].id);
        return;
      }
      const cols = grid.current ? getComputedStyle(grid.current).gridTemplateColumns.split(" ").length : 1;
      const next = gridMove(focus, action, cols, shown.length);
      if (next < 0 || next === focus) return;
      setNavigating(true);
      setFocus(next);
      // With the panel open it follows the focus.
      if (open && shown[next]) h.openGame(shown[next].id);
    });
    const slash = (e: KeyboardEvent) => {
      if (e.key === "/" && !(e.target instanceof HTMLInputElement)) {
        e.preventDefault();
        setFiltersOpen(true);
      }
    };
    addEventListener("keydown", slash);
    return () => {
      nav.stop();
      removeEventListener("keydown", slash);
    };
  }, []);

  if (!system) return null;
  const filterCount = activeFilters(filters) + (query.trim() ? 1 : 0);
  const showJump = sort === "title" && shown.length > 12 && stops.length > 1;
  const auto = slots.some((s) => s.slot === AUTO_SLOT);

  return (
    <>
      <div
        className="fixed inset-0 -z-10"
        style={{ background: `radial-gradient(ellipse at 0% 0%, color-mix(in srgb, ${system.color} 25%, transparent), #0b0d12 55%)` }}
      />
      {/* The panel floats over the grid; the grid never reflows for it. */}
      <main className="px-4 pt-10 pb-24 sm:px-8">
        {/* The header: the system, and the chips beside it where there is room. */}
        <div className="mb-6 flex flex-col gap-4 pt-4 lg:flex-row lg:items-center lg:justify-between">
          <h1 className="flex flex-wrap items-baseline gap-3 text-3xl font-bold tracking-tight">
            <span className="self-center rounded-lg px-2.5 py-1 text-sm font-black" style={{ background: system.color }}>
              {system.short}
            </span>
            {system.name}
            <span className="text-sm font-normal text-slate-400">
              {shown.length === games.length ? `${games.length} games` : `${shown.length} of ${games.length}`}
            </span>
          </h1>
          <div className="flex min-w-0">
            <TabStrip label="Sort, filter and jump">
              <SortChip sort={sort} onSort={changeSort} />
              <TabStripDivider />
              <button
                type="button"
                onClick={() => setFiltersOpen(true)}
                aria-pressed={filterCount > 0}
                className={filterCount > 0 ? CHIP_ON : CHIP}
              >
                <SlidersHorizontal /> {filterCount > 0 ? `Filters (${filterCount})` : "Filters"}
              </button>
              {showJump && <JumpButton onClick={openJump} />}
            </TabStrip>
          </div>
        </div>

        {shown.length ? (
          <div ref={grid} className="grid grid-cols-[repeat(auto-fill,minmax(9.5rem,1fr))] gap-x-4 gap-y-6">
            {shown.map((g, i) => (
              <GameCard
                key={g.id}
                game={g}
                system={system}
                focused={navigating && i === focus}
                open={g.id === openId}
                onOpen={() => {
                  setFocus(i);
                  openGame(g.id);
                }}
              />
            ))}
          </div>
        ) : (
          <div className="mx-auto max-w-lg rounded-xl border border-dashed border-white/15 bg-white/5 py-20 text-center text-slate-400">
            {games.length ? "No games match. Try fewer words, or clear the filters." : "No games for this system yet."}
          </div>
        )}
      </main>

      <GamePanel game={open} system={system} slots={slots} onClose={closePanel} onPlay={(start) => play(open, start)} />
      <FilterDrawer
        open={filtersOpen}
        onClose={() => setFiltersOpen(false)}
        games={games}
        query={query}
        onQuery={setQuery}
        filters={filters}
        onFilters={setFilters}
        searchRef={search}
      />
      {jump && (
        <JumpStrip
          stops={stops}
          index={jump.index}
          onIndex={(index) => setJump({ ...jump, index })}
          onCommit={commitJump}
          onCancel={cancelJump}
        />
      )}
      <HintBar
        hints={
          open
            ? [
                { button: "dpad", label: "Choose" },
                { button: "a", label: auto ? "Continue" : "Play" },
                { button: "b", label: "Close" },
              ]
            : [
                { button: "dpad", label: "Choose" },
                { button: "a", label: "Open" },
                { button: "start", label: "Filters" },
                { button: "b", label: "Home" },
              ]
        }
      />
    </>
  );
}

export default function SystemPage() {
  return (
    <Shell>
      <SideNav section="systems" />
      <div className={NAV_SPACE}>
        <Games />
      </div>
    </Shell>
  );
}
