// A system's game list: searching, filtering, sorting and the A–Z jump, kept here so the page
// only draws what these return.
import type { EsrbRating, Game } from "./api";
import { ESRB_ORDER } from "./esrb";

export type GameSort = "title" | "recent" | "year" | "plays" | "rating" | "age";

export const SORTS: { value: GameSort; label: string }[] = [
  { value: "title", label: "Title" },
  { value: "recent", label: "Recently played" },
  { value: "year", label: "Release year" },
  { value: "plays", label: "Most played" },
  { value: "rating", label: "Highest rated" },
  { value: "age", label: "Age rating" },
];

/** What the list is narrowed to. A list filter keeps games with any of its values; filters
 *  combine (a game must pass every one). */
export interface GameFilters {
  played?: "played" | "unplayed";
  /** Only this user's favourites. */
  favourites?: boolean;
  /** Only games for at least this many players. */
  minPlayers?: number;
  /** Only games rated at least this many stars out of 5. */
  minStars?: number;
  /** Only games in any of these genres ("Platform", "Shooter" …). */
  genres?: string[];
  /** Only games with any of these ESRB ratings. */
  esrb?: EsrbRating[];
  /** Only games with any of these ways to play ("Single player", "Co-op" …). */
  modes?: string[];
  /** Only games in any of these series ("Castlevania" …). */
  series?: string[];
  /** Only games with any of these themes ("Horror" …). */
  themes?: string[];
}

/** The list filters, each with what it reads from a game, as the drawer offers them. */
export const LIST_FILTERS = [
  { key: "genres", label: "Genre", values: (g: Game) => genresOf(g) },
  { key: "modes", label: "Play modes", values: (g: Game) => g.modes },
  { key: "series", label: "Series", values: (g: Game) => g.series },
  { key: "themes", label: "Themes", values: (g: Game) => g.themes },
] as const satisfies readonly { key: keyof GameFilters; label: string; values: (g: Game) => string[] }[];

/** A game's genres: the metadata lists them comma-separated ("Platform, Action"). */
export const genresOf = (g: Pick<Game, "genre">) =>
  (g.genre ?? "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);

/** The most players a game takes, from "1-2", "2", "1 to 4" …, or null when unknown. */
export function maxPlayers(players: string | null): number | null {
  const numbers = (players ?? "").match(/\d+/g)?.map(Number) ?? [];
  return numbers.length ? Math.max(...numbers) : null;
}

/** Folds case and accents, so "pokemon" finds "Pokémon". */
const fold = (s: string) =>
  s
    .normalize("NFD")
    .replace(/\p{Diacritic}/gu, "")
    .toLowerCase();

/** Games matching every word of `query` (in the title, file name, genre, maker, tags, series or
 *  themes) and every filter. */
export function filterGames(games: Game[], query: string, filters: GameFilters): Game[] {
  const words = fold(query).split(/\s+/).filter(Boolean);
  return games.filter((g) => {
    if (filters.played === "played" && !g.playCount) return false;
    if (filters.played === "unplayed" && g.playCount) return false;
    if (filters.favourites && !g.favourite) return false;
    if (filters.minPlayers && (maxPlayers(g.players) ?? 1) < filters.minPlayers) return false;
    if (filters.minStars && (g.rating === null || g.rating * 5 < filters.minStars - 0.001)) return false;
    if (filters.esrb?.length && !(g.esrb && filters.esrb.includes(g.esrb))) return false;
    for (const { key, values } of LIST_FILTERS) {
      const wanted = filters[key];
      if (wanted?.length && !values(g).some((v) => wanted.includes(v))) return false;
    }
    if (!words.length) return true;
    const text = fold(
      [g.title, g.fileTitle, g.genre, g.developer, g.publisher, ...g.tags, ...g.series, ...g.themes]
        .filter(Boolean)
        .join(" "),
    );
    return words.every((w) => text.includes(w));
  });
}

/** Where a game's ESRB rating falls from the youngest audience to the oldest; unrated last. */
const ageRank = (g: Game) => (g.esrb ? ESRB_ORDER.indexOf(g.esrb) : ESRB_ORDER.length);

/** Sorted by `sort`; ties (and the title sort itself) go by title. */
export function sortGames(games: Game[], sort: GameSort): Game[] {
  const byTitle = (a: Game, b: Game) =>
    a.title.localeCompare(b.title, undefined, { sensitivity: "base", numeric: true });
  const year = (g: Game) => Number(g.releaseDate?.slice(0, 4)) || 0;
  const played = (g: Game) => (g.lastPlayedAt ? Date.parse(`${g.lastPlayedAt.replace(" ", "T")}Z`) : 0);
  const key: Record<GameSort, (a: Game, b: Game) => number> = {
    title: () => 0,
    recent: (a, b) => played(b) - played(a),
    year: (a, b) => (year(a) || 9999) - (year(b) || 9999),
    plays: (a, b) => (b.playCount ?? 0) - (a.playCount ?? 0),
    // Unrated games last, whatever the order.
    rating: (a, b) => (b.rating ?? -1) - (a.rating ?? -1),
    age: (a, b) => ageRank(a) - ageRank(b),
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

/** Every value `values` gives the games, with how many games have it, most common first, for a
 *  filter's choices. Empty until game information has been fetched. */
export function valueCounts(games: Game[], values: (g: Game) => readonly string[]): { value: string; count: number }[] {
  const counts = new Map<string, number>();
  for (const g of games) for (const x of new Set(values(g))) counts.set(x, (counts.get(x) ?? 0) + 1);
  return [...counts]
    .map(([value, count]) => ({ value, count }))
    .sort((a, b) => b.count - a.count || a.value.localeCompare(b.value));
}

/** The ESRB ratings the games have, youngest audience first, with how many have each. */
export function esrbCounts(games: Game[]): { value: EsrbRating; count: number }[] {
  const counts = new Map<EsrbRating, number>();
  for (const g of games) if (g.esrb) counts.set(g.esrb, (counts.get(g.esrb) ?? 0) + 1);
  return ESRB_ORDER.filter((r) => counts.has(r)).map((value) => ({ value, count: counts.get(value) ?? 0 }));
}

/** Whether any game has a rating, so the minimum-rating filter can do something. */
export const knowsRatings = (games: Game[]) => games.some((g) => g.rating !== null);

/** Whether any game says how many players it takes, so the players filter can do something. */
export const knowsPlayers = (games: Game[]) => games.some((g) => maxPlayers(g.players) !== null);

/** How many filters are on, for the filter button's badge. */
export const activeFilters = (f: GameFilters) =>
  (f.played ? 1 : 0) +
  (f.favourites ? 1 : 0) +
  (f.minPlayers ? 1 : 0) +
  (f.minStars ? 1 : 0) +
  (f.esrb?.length ?? 0) +
  LIST_FILTERS.reduce((n, { key }) => n + (f[key]?.length ?? 0), 0);

// --- The grid ---------------------------------------------------------------------------------

/** Where a pad or arrow key moves the focus in a grid of `count` games, `cols` wide: left and
 *  right along the row (wrapping onto the next or previous row), up and down a row, the
 *  shoulders three rows. Stops at the ends. */
export function gridMove(index: number, action: string, cols: number, count: number): number {
  if (count <= 0) return -1;
  const step: Record<string, number> = {
    left: -1,
    right: 1,
    up: -cols,
    down: cols,
    pageUp: -3 * cols,
    pageDown: 3 * cols,
  };
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

/** A multi-part game's count in words: "3 discs", "2 disks", "2 sides" (discs when unknown). */
export function partsLabel(count: number, kind: string | null | undefined): string {
  const nouns: Record<string, string> = { DISC: "disc", DISK: "disk", TAPE: "tape", SIDE: "side", PART: "part" };
  const noun = nouns[kind ?? "DISC"] ?? "disc";
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}
