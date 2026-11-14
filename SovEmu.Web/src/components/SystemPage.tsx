// One system's games: a searchable, sortable, filterable list on the left (with an A–Z strip),
// the chosen game's details on the right (artwork, facts, description, Continue / Start over,
// save slots). Pad and keyboard: up/down choose, left/right (and the shoulder buttons) page,
// A plays, Start searches, B goes home. The list logic is src/lib/games.ts.
import {
  Building2,
  CalendarDays,
  Check,
  CircleCheck,
  ListFilter,
  Play,
  RotateCcw,
  Search,
  Tag,
  Users,
  X,
} from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  AUTO_SLOT,
  api,
  formatAgo,
  formatSize,
  mediaUrl,
  screenshotUrl,
  type Game,
  type SaveSlot,
  type SystemSummary,
} from "../lib/api";
import {
  LETTERS,
  SORTS,
  activeFilters,
  filterGames,
  jumpTo,
  letterOf,
  maxPlayers,
  sortGames,
  tagCounts,
  tagsOf,
  type GameFilters,
  type GameSort,
} from "../lib/games";
import { listenNav } from "../lib/nav";
import HintBar from "./HintBar";
import Shell from "./Shell";
import SideNav, { NAV_SPACE } from "./SideNav";
import { Button } from "./ui/button";
import { cn } from "./ui/cn";
import { Select, Tooltip } from "./ui/primitives";

const PAGE = 10;
const SORT_KEY = "sovemu.sort";

/** A small cover for a list row, or the system's colour where there is no art. */
function Thumb({ game, color, short }: { game: Game; color: string; short: string }) {
  const kind = game.media.includes("thumb") ? "thumb" : game.media.includes("image") ? "image" : null;
  return (
    <span
      className="relative grid h-9 w-12 shrink-0 place-items-center overflow-hidden rounded text-[9px] font-black text-white/70"
      style={{ background: `linear-gradient(145deg, ${color}, color-mix(in srgb, ${color} 30%, black))` }}
    >
      {short}
      {kind && <img src={mediaUrl(game.id, kind)} alt="" loading="lazy" className="absolute inset-0 size-full object-cover" />}
    </span>
  );
}

/** The details pane's picture: the screenshot (or box art), then its video snap after a pause,
 *  with the box art over a corner when there is both. */
function Art({ game, color, short }: { game: Game; color: string; short: string }) {
  const [video, setVideo] = useState(false);
  const [playing, setPlaying] = useState(false);
  useEffect(() => {
    setVideo(false);
    setPlaying(false);
    if (!game.media.includes("video")) return;
    const t = setTimeout(() => setVideo(true), 900);
    return () => clearTimeout(t);
  }, [game.id]);
  const still = game.media.includes("image") ? "image" : game.media.includes("thumb") ? "thumb" : null;
  const boxArt = still === "image" && game.media.includes("thumb");
  return (
    <div
      className="relative grid aspect-video shrink-0 place-items-center overflow-hidden rounded-t-2xl text-6xl font-black text-white/80"
      style={{ background: `linear-gradient(145deg, ${color}, color-mix(in srgb, ${color} 30%, black))` }}
    >
      <span>{short}</span>
      {still && (
        <img
          key={game.id}
          src={mediaUrl(game.id, still)}
          alt=""
          className={cn("absolute inset-0 size-full", still === "image" ? "object-cover" : "object-contain")}
        />
      )}
      {video && (
        <video
          src={mediaUrl(game.id, "video")}
          autoPlay
          muted
          loop
          playsInline
          onPlaying={() => setPlaying(true)}
          className={cn("absolute inset-0 size-full object-cover", !playing && "hidden")}
        />
      )}
      <div className="absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-panel to-transparent" />
      {boxArt && (
        <img src={mediaUrl(game.id, "thumb")} alt="" className="absolute bottom-3 left-4 h-1/2 rounded-md object-contain shadow-2xl" />
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

/** Played, players and tags: a menu of checkboxes that stays open while ticking. */
function FilterMenu({ games, filters, onChange }: { games: Game[]; filters: GameFilters; onChange: (f: GameFilters) => void }) {
  const tags = useMemo(() => tagCounts(games).slice(0, 12), [games]);
  const n = activeFilters(filters);
  const item =
    "relative flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 pl-7 text-sm text-slate-200 outline-none select-none data-[highlighted]:bg-white/10";
  const check = (
    <DropdownMenu.ItemIndicator className="absolute left-2">
      <Check className="size-3.5" />
    </DropdownMenu.ItemIndicator>
  );
  const label = "px-2 pt-2 pb-1 text-[11px] font-semibold tracking-wider text-slate-500 uppercase";
  const toggleTag = (t: string) => {
    const has = filters.tags?.includes(t);
    onChange({ ...filters, tags: has ? filters.tags!.filter((x) => x !== t) : [...(filters.tags ?? []), t] });
  };
  const box = (checked: boolean, onCheck: (on: boolean) => void, children: ReactNode, key?: string) => (
    <DropdownMenu.CheckboxItem
      key={key}
      className={item}
      checked={checked}
      onSelect={(e) => e.preventDefault()}
      onCheckedChange={(on) => onCheck(on === true)}
    >
      {check}
      {children}
    </DropdownMenu.CheckboxItem>
  );
  return (
    <DropdownMenu.Root modal={false}>
      <DropdownMenu.Trigger asChild>
        <Button variant="outline" className={cn("h-9", n > 0 && "border-accent text-white")}>
          <ListFilter /> Filter
          {n > 0 && <span className="rounded-full bg-accent px-1.5 text-[11px] leading-4">{n}</span>}
        </Button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          align="end"
          sideOffset={6}
          collisionPadding={8}
          className="z-50 max-h-[70dvh] w-56 overflow-y-auto rounded-lg border border-edge bg-panel p-1 shadow-2xl"
        >
          <DropdownMenu.Label className={label}>Played</DropdownMenu.Label>
          {box(filters.played === "played", (on) => onChange({ ...filters, played: on ? "played" : undefined }), "Played before")}
          {box(filters.played === "unplayed", (on) => onChange({ ...filters, played: on ? "unplayed" : undefined }), "Not played yet")}
          <DropdownMenu.Label className={label}>Players</DropdownMenu.Label>
          {box(!!filters.multiplayer, (on) => onChange({ ...filters, multiplayer: on || undefined }), "2 players or more")}
          {tags.length > 0 && <DropdownMenu.Label className={label}>Tags</DropdownMenu.Label>}
          {tags.map(({ tag, count }) =>
            box(
              !!filters.tags?.includes(tag),
              () => toggleTag(tag),
              <>
                <span className="flex-1 truncate">{tag}</span>
                <span className="text-xs text-slate-500 tabular-nums">{count}</span>
              </>,
              tag,
            ),
          )}
          {n > 0 && (
            <>
              <DropdownMenu.Separator className="my-1 h-px bg-white/10" />
              <DropdownMenu.Item className={cn(item, "pl-2 text-slate-400")} onSelect={() => onChange({})}>
                <X className="size-3.5" /> Clear filters
              </DropdownMenu.Item>
            </>
          )}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

function Games() {
  const systemId = new URLSearchParams(location.search).get("id") ?? "";
  const rememberKey = `sovemu.game.${systemId}`;
  const [system, setSystem] = useState<SystemSummary | null>(null);
  const [games, setGames] = useState<Game[]>([]);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<GameSort>(() => (localStorage.getItem(SORT_KEY) as GameSort) || "title");
  const [filters, setFilters] = useState<GameFilters>({});
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [slots, setSlots] = useState<SaveSlot[]>([]);
  const list = useRef<HTMLOListElement>(null);
  const search = useRef<HTMLInputElement>(null);

  useEffect(() => {
    Promise.all([api.systems(), api.games(systemId)]).then(([systems, games]) => {
      const s = systems.find((x) => x.id === systemId);
      if (!s) return location.replace("/");
      document.title = `${s.name} · SovEmu`;
      setSystem(s);
      setGames(games);
      const remembered = Number(localStorage.getItem(rememberKey));
      setSelectedId(games.some((g) => g.id === remembered) ? remembered : null);
    });
  }, []);

  const shown = useMemo(() => sortGames(filterGames(games, query, filters), sort), [games, query, filters, sort]);
  const index = Math.max(0, shown.findIndex((g) => g.id === selectedId));
  const game = shown[index];
  const letters = useMemo(() => new Set(shown.map((g) => letterOf(g.title))), [shown]);

  const select = (i: number) => {
    const g = shown[Math.min(Math.max(i, 0), shown.length - 1)];
    if (g) setSelectedId(g.id);
  };

  // Remember the choice, keep it in view, and read its saves (after a pause, so scrolling past
  // games does not ask for each one).
  useEffect(() => {
    if (!game) return;
    localStorage.setItem(rememberKey, String(game.id));
    list.current?.querySelector(`[data-id="${game.id}"]`)?.scrollIntoView({ block: "nearest" });
    setSlots([]);
    const t = setTimeout(() => api.states(game.id).then(setSlots).catch(() => {}), 200);
    return () => clearTimeout(t);
  }, [game?.id]);

  const auto = slots.find((s) => s.slot === AUTO_SLOT);
  const play = (g: Game | undefined, start: "continue" | "new") => {
    if (g) location.href = `/play/?game=${g.id}&start=${start}`;
  };

  const latest = useRef({ index, shown, auto, game });
  latest.current = { index, shown, auto, game };
  useEffect(() => {
    const nav = listenNav((action) => {
      const { index, shown, auto, game } = latest.current;
      const to = (i: number) => {
        const g = shown[Math.min(Math.max(i, 0), shown.length - 1)];
        if (g) setSelectedId(g.id);
      };
      if (action === "up") to(index - 1);
      if (action === "down") to(index + 1);
      if (action === "pageUp" || action === "left") to(index - PAGE);
      if (action === "pageDown" || action === "right") to(index + PAGE);
      if (action === "accept") play(game, auto ? "continue" : "new");
      if (action === "menu") search.current?.focus();
      if (action === "back") location.href = "/";
    });
    // "/" searches, as in most lists.
    const slash = (e: KeyboardEvent) => {
      if (e.key === "/" && document.activeElement !== search.current) {
        e.preventDefault();
        search.current?.focus();
      }
    };
    addEventListener("keydown", slash);
    return () => {
      nav.stop();
      removeEventListener("keydown", slash);
    };
  }, []);

  if (!system) return null;
  const color = system.color;
  const year = game?.release_date?.slice(0, 4);
  const players = maxPlayers(game?.players ?? null);
  const filtered = shown.length !== games.length;

  return (
    <>
      <div
        className="fixed inset-0 -z-10"
        style={{ background: `radial-gradient(ellipse at 0% 0%, color-mix(in srgb, ${color} 30%, transparent), #0b0d12 60%)` }}
      />
      <main className="mx-auto grid h-dvh max-w-7xl grid-rows-[auto_minmax(0,1fr)] gap-5 px-6 pt-16 pb-20 md:px-8">
        <h1 className="flex flex-wrap items-baseline gap-3">
          <span className="rounded-lg px-2.5 py-1 text-sm font-black" style={{ background: color }}>
            {system.short}
          </span>
          <span className="text-2xl font-bold">{system.name}</span>
          <span className="text-sm text-slate-400">
            {filtered ? `${shown.length} of ${games.length} games` : `${games.length} game${games.length === 1 ? "" : "s"}`}
          </span>
        </h1>

        <div className="grid min-h-0 grid-cols-1 gap-6 md:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)]">
          {/* The list: search, sort, filter, rows, and the A–Z strip. */}
          <section className="flex min-h-0 flex-col rounded-2xl border border-white/10 bg-black/30">
            <div className="flex flex-wrap items-center gap-2 border-b border-white/10 p-3">
              <label className="relative min-w-40 flex-1">
                <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-slate-500" />
                <input
                  ref={search}
                  type="search"
                  value={query}
                  placeholder={`Search ${system.short} games  ( / )`}
                  aria-label="Search games"
                  onChange={(e) => setQuery(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === "ArrowDown") {
                      e.preventDefault();
                      search.current?.blur();
                    }
                    if (e.key === "Escape") {
                      setQuery("");
                      search.current?.blur();
                    }
                  }}
                  className="h-9 w-full rounded-md border border-edge bg-black/30 pr-3 pl-8 text-sm outline-none focus:border-accent"
                />
              </label>
              <Select
                label="Sort"
                value={sort}
                options={SORTS}
                onChange={(v) => {
                  setSort(v as GameSort);
                  localStorage.setItem(SORT_KEY, v);
                }}
                className="h-9 min-w-36"
              />
              <FilterMenu games={games} filters={filters} onChange={setFilters} />
            </div>

            <ol ref={list} className="min-h-0 flex-1 overflow-y-auto p-2" aria-label="Games">
              {shown.map((g) => {
                const on = g.id === game?.id;
                const n = maxPlayers(g.players);
                const tags = tagsOf(g);
                return (
                  <li
                    key={g.id}
                    data-id={g.id}
                    aria-selected={on}
                    onClick={() => (on ? play(g, auto ? "continue" : "new") : setSelectedId(g.id))}
                    className={cn("flex cursor-pointer items-center gap-3 rounded-lg px-2 py-1.5", on ? "text-white" : "text-slate-300 hover:bg-white/5")}
                    style={on ? { background: color } : undefined}
                  >
                    <Thumb game={g} color={color} short={system.short} />
                    <div className="min-w-0 flex-1">
                      <p className={cn("truncate", on && "font-semibold")}>{g.title}</p>
                      {tags.length > 0 && (
                        <p className={cn("truncate text-xs", on ? "text-white/75" : "text-slate-500")}>{tags.join(" · ")}</p>
                      )}
                    </div>
                    <span className={cn("flex shrink-0 items-center gap-2 text-xs tabular-nums", on ? "text-white/85" : "text-slate-500")}>
                      {!!g.play_count && <CircleCheck className="size-3.5" aria-label="Played" />}
                      {n !== null && n >= 2 && (
                        <span className="flex items-center gap-0.5">
                          <Users className="size-3.5" />
                          {n}
                        </span>
                      )}
                      {g.release_date && <span>{g.release_date.slice(0, 4)}</span>}
                    </span>
                  </li>
                );
              })}
              {!shown.length && (
                <li className="px-3 py-8 text-center text-sm text-slate-400">
                  {games.length ? "No games match. Try fewer words or clear the filters." : "No games for this system yet."}
                </li>
              )}
            </ol>

            {sort === "title" && shown.length > PAGE && (
              <nav aria-label="Jump to letter" className="grid grid-cols-[repeat(27,minmax(0,1fr))] border-t border-white/10 px-2 py-1.5">
                {LETTERS.map((l) => (
                  <button
                    key={l}
                    type="button"
                    disabled={!letters.has(l)}
                    onClick={() => select(jumpTo(shown, l))}
                    className={cn(
                      "h-6 rounded text-[11px] font-semibold text-slate-400 hover:bg-white/10 hover:text-white disabled:pointer-events-none disabled:opacity-25",
                      game && letterOf(game.title) === l && "bg-white/10 text-white",
                    )}
                  >
                    {l}
                  </button>
                ))}
              </nav>
            )}
          </section>

          {/* The chosen game. */}
          {game && (
            <aside className="hidden min-h-0 flex-col overflow-y-auto rounded-2xl border border-white/10 bg-panel/80 shadow-2xl backdrop-blur md:flex">
              <Art game={game} color={color} short={system.short} />
              <div className="grid gap-4 p-6 pt-4">
                <div>
                  <h2 className="text-3xl leading-tight font-black">{game.title}</h2>
                  <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1">
                    {year && <Fact icon={<CalendarDays />}>{year}</Fact>}
                    {(game.developer || game.publisher) && <Fact icon={<Building2 />}>{game.developer ?? game.publisher}</Fact>}
                    {game.genre && <Fact icon={<Tag />}>{game.genre}</Fact>}
                    {players !== null && <Fact icon={<Users />}>{players === 1 ? "1 player" : `${game.players} players`}</Fact>}
                  </div>
                </div>
                {game.description && <p className="line-clamp-4 text-sm leading-relaxed text-slate-300">{game.description}</p>}

                <div className="flex flex-wrap gap-2">
                  {auto ? (
                    <>
                      <Button size="lg" onClick={() => play(game, "continue")}>
                        <Play className="fill-current" /> Continue
                      </Button>
                      <Button size="lg" variant="outline" onClick={() => play(game, "new")}>
                        <RotateCcw /> Start over
                      </Button>
                    </>
                  ) : (
                    <Button size="lg" onClick={() => play(game, "new")}>
                      <Play className="fill-current" /> Play
                    </Button>
                  )}
                </div>

                {slots.length > 0 && (
                  <div>
                    <p className="mb-2 text-xs font-semibold tracking-wider text-slate-500 uppercase">Saves</p>
                    <div className="flex flex-wrap gap-2">
                      {slots.map((s) => (
                        <Tooltip key={s.slot} content={`${s.slot === AUTO_SLOT ? "Auto save" : `Slot ${s.slot}`} · ${ago(s.saved_at)}`}>
                          <span className="pixelated grid h-12 w-16 place-items-center overflow-hidden rounded border border-white/10 bg-black/50 text-[10px] text-slate-400">
                            {s.screenshot ? (
                              <img src={screenshotUrl(game.id, s)} alt="" className="pixelated size-full object-cover" />
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
                  {game.play_count ? `Played ${game.play_count}× · last ${formatAgo(game.last_played_at)}` : "Not played yet"}
                  {" · "}
                  <span className="font-mono">{game.path}</span> · {formatSize(game.size)}
                </p>
              </div>
            </aside>
          )}
        </div>
      </main>
      <HintBar
        hints={[
          { button: "dpad", label: "Choose" },
          { button: "a", label: auto ? "Continue" : "Play" },
          { button: "start", label: "Search" },
          { button: "b", label: "Home" },
        ]}
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
