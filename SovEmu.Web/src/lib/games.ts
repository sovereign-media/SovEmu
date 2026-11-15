// A system's game list: searching, filtering, sorting and the A–Z jump, kept here so the page
// only draws what these return.
import type { Game } from "./api";

export type GameSort = "title" | "recent" | "year" | "plays";

export const SORTS: { value: GameSort; label: string }[] = [
  { value: "title", label: "Title" },
  { value: "recent", label: "Recently played" },
  { value: "year", label: "Release year" },
  { value: "plays", label: "Most played" },
];

export interface GameFilters {
  played?: "played" | "unplayed";
  /** Only games for at least two players. */
  multiplayer?: boolean;
  /** Only games with every one of these tags ("USA", "Rev 1" …). */
  tags?: string[];
}

/** The tags of a game ("USA", "Europe" …). */
export const tagsOf = (g: Pick<Game, "tags">) => g.tags;

/** The most players a game takes, from "1-2", "2", "1 to 4" …, or null when unknown. */
export function maxPlayers(players: string | null): number | null {
  const numbers = (players ?? "").match(/\d+/g)?.map(Number) ?? [];
  return numbers.length ? Math.max(...numbers) : null;
}

/** Folds case and accents, so "pokemon" finds "Pokémon". */
const fold = (s: string) => s.normalize("NFD").replace(/\p{Diacritic}/gu, "").toLowerCase();

/** Games matching every word of `query` (in the title, file name, genre, maker or tags) and
 *  every filter. */
export function filterGames(games: Game[], query: string, filters: GameFilters): Game[] {
  const words = fold(query).split(/\s+/).filter(Boolean);
  return games.filter((g) => {
    if (filters.played === "played" && !g.playCount) return false;
    if (filters.played === "unplayed" && g.playCount) return false;
    if (filters.multiplayer && (maxPlayers(g.players) ?? 1) < 2) return false;
    if (filters.tags?.length) {
      const tags = tagsOf(g);
      if (!filters.tags.every((t) => tags.includes(t))) return false;
    }
    if (!words.length) return true;
    const text = fold([g.title, g.fileTitle, g.genre, g.developer, g.publisher, ...g.tags].filter(Boolean).join(" "));
    return words.every((w) => text.includes(w));
  });
}

/** Sorted by `sort`; ties (and the title sort itself) go by title. */
export function sortGames(games: Game[], sort: GameSort): Game[] {
  const byTitle = (a: Game, b: Game) => a.title.localeCompare(b.title, undefined, { sensitivity: "base", numeric: true });
  const year = (g: Game) => Number(g.releaseDate?.slice(0, 4)) || 0;
  const played = (g: Game) => (g.lastPlayedAt ? Date.parse(g.lastPlayedAt.replace(" ", "T") + "Z") : 0);
  const key: Record<GameSort, (a: Game, b: Game) => number> = {
    title: () => 0,
    recent: (a, b) => played(b) - played(a),
    year: (a, b) => (year(a) || 9999) - (year(b) || 9999),
    plays: (a, b) => (b.playCount ?? 0) - (a.playCount ?? 0),
  };
  return [...games].sort((a, b) => key[sort](a, b) || byTitle(a, b));
}

/** The letter a title files under in the A–Z strip: A–Z, or "#" for digits and symbols. Leading
 *  articles are not skipped ("The Legend of Zelda" is under T), as in the list itself. */
export function letterOf(title: string): string {
  const c = fold(title).charAt(0).toUpperCase();
  return c >= "A" && c <= "Z" ? c : "#";
}

/** The letters the strip offers: "#" then A–Z. */
export const LETTERS = ["#", ..."ABCDEFGHIJKLMNOPQRSTUVWXYZ"];

/** Where the A–Z strip jumps for `letter`: the first game filed under it, else the next letter
 *  that has one, else the last game; -1 for an empty list. */
export function jumpTo(games: Game[], letter: string): number {
  if (!games.length) return -1;
  const wanted = LETTERS.indexOf(letter);
  const at = games.findIndex((g) => LETTERS.indexOf(letterOf(g.title)) >= wanted);
  return at >= 0 ? at : games.length - 1;
}

/** Every tag in the list with how many games carry it, most common first, for the filter menu. */
export function tagCounts(games: Game[]): { tag: string; count: number }[] {
  const counts = new Map<string, number>();
  for (const g of games) for (const t of tagsOf(g)) counts.set(t, (counts.get(t) ?? 0) + 1);
  return [...counts].map(([tag, count]) => ({ tag, count })).sort((a, b) => b.count - a.count || a.tag.localeCompare(b.tag));
}

/** How many filters are on, for the filter button's badge. */
export const activeFilters = (f: GameFilters) => (f.played ? 1 : 0) + (f.multiplayer ? 1 : 0) + (f.tags?.length ?? 0);

// --- The grid ---------------------------------------------------------------------------------

/** Where a pad or arrow key moves the focus in a grid of `count` games, `cols` wide: left and
 *  right along the row (wrapping onto the next or previous row), up and down a row, the
 *  shoulders three rows. Stops at the ends. */
export function gridMove(index: number, action: string, cols: number, count: number): number {
  if (count <= 0) return -1;
  const step: Record<string, number> = { left: -1, right: 1, up: -cols, down: cols, pageUp: -3 * cols, pageDown: 3 * cols };
  const delta = step[action];
  if (delta === undefined) return index;
  const next = index + delta;
  // Down from the last full row lands on the last game rather than nowhere.
  if (action === "down" || action === "pageDown") return Math.min(next, count - 1);
  return Math.max(0, Math.min(next, count - 1));
}

// --- The jump strip (Sovereign's letter carousel) -----------------------------------------------

/** How many letters the strip shows either side of the current one. */
export const STRIP_REACH = 3;

/** The strip's stops: only letters some shown game files under, "#" first. */
export function jumpStops(games: Pick<Game, "title">[]): string[] {
  const present = new Set(games.map((g) => letterOf(g.title)));
  return LETTERS.filter((l) => present.has(l));
}

/** The current stop and up to `reach` either side of it. */
export function stripWindow(stops: string[], index: number, reach = STRIP_REACH) {
  return {
    before: stops.slice(Math.max(0, index - reach), index),
    current: stops[index] ?? "",
    after: stops.slice(index + 1, index + 1 + reach),
  };
}

/** One step left or right along the strip, clamped (it does not wrap). */
export const stepStop = (stops: string[], index: number, delta: -1 | 1) =>
  Math.min(stops.length - 1, Math.max(0, index + delta));

// --- The panel's URL ----------------------------------------------------------------------------

/** The game a URL's `?game=` opens, if any. */
export function gameFromUrl(search: string): number | null {
  const n = Number(new URLSearchParams(search).get("game"));
  return Number.isInteger(n) && n > 0 ? n : null;
}

/** `href` with `?game=` set to `id`, or removed for null; every other parameter is kept. */
export function urlWithGame(href: string, id: number | null): string {
  const url = new URL(href);
  if (id === null) url.searchParams.delete("game");
  else url.searchParams.set("game", String(id));
  return url.pathname + url.search + url.hash;
}
