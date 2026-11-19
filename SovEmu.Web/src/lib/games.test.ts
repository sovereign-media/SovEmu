import { describe, expect, it } from "vitest";
import type { Game } from "./api";
import { activeFilters, filterGames, gameFromUrl, genreCounts, gridMove, jumpStops, jumpTo, knowsPlayers, letterOf, maxPlayers, sortGames, stepStop, stripWindow, urlWithGame } from "./games";

let nextId = 1;
const game = (title: string, over: Partial<Game> = {}): Game => ({
  id: nextId++, system: "nes", title, tags: [], path: `${title}.nes`, size: 1, lastPlayedAt: null, playCount: 0,
  image: null, thumb: null, video: null, fileTitle: null, description: null, releaseDate: null, developer: null,
  publisher: null, genre: null, players: null, rating: null, favourite: false, discs: 1, ...over,
});

const metroid = game("Metroid", { releaseDate: "1986-08-06", genre: "Action", players: "1", playCount: 3, lastPlayedAt: "2026-11-13 10:00:00", tags: ["USA"] });
const mario = game("Super Mario Bros.", { releaseDate: "1985-09-13", genre: "Platform", players: "1-2", playCount: 1, lastPlayedAt: "2026-11-14 09:00:00", tags: ["USA", "Europe"] });
const pokemon = game("Pokémon Puzzle", { players: "2", genre: "Puzzle, Action", tags: ["Europe"] });
const tetris = game("Tetris", { releaseDate: "1989" });
const all = [tetris, pokemon, mario, metroid];

describe("searching and filtering", () => {
  it("matches every word, ignoring case and accents", () => {
    expect(filterGames(all, "mario", {}).map((g) => g.title)).toEqual(["Super Mario Bros."]);
    expect(filterGames(all, "POKEMON", {})).toHaveLength(1);
    expect(filterGames(all, "action metroid", {})).toHaveLength(1);
    expect(filterGames(all, "europe", {}).map((g) => g.title)).toEqual(["Pokémon Puzzle", "Super Mario Bros."]);
    expect(filterGames(all, "", {})).toHaveLength(4);
  });

  it("filters by played, players and genres", () => {
    expect(filterGames(all, "", { played: "played" }).map((g) => g.title).sort()).toEqual(["Metroid", "Super Mario Bros."]);
    expect(filterGames(all, "", { played: "unplayed" })).toHaveLength(2);
    expect(filterGames(all, "", { multiplayer: true }).map((g) => g.title).sort()).toEqual(["Pokémon Puzzle", "Super Mario Bros."]);
    expect(filterGames(all, "", { genres: ["Action"] }).map((g) => g.title)).toEqual(["Pokémon Puzzle", "Metroid"]);
    expect(filterGames(all, "", { genres: ["Puzzle", "Platform"] }).map((g) => g.title)).toEqual(["Pokémon Puzzle", "Super Mario Bros."]);
    expect(activeFilters({ played: "played", multiplayer: true, genres: ["Action"] })).toBe(3);
  });

  it("reads the most players from the metadata's wording", () => {
    expect(maxPlayers("1-2")).toBe(2);
    expect(maxPlayers("1 to 4")).toBe(4);
    expect(maxPlayers(null)).toBeNull();
    expect(knowsPlayers(all)).toBe(true);
    expect(knowsPlayers([tetris])).toBe(false);
  });
});

describe("sorting", () => {
  const titles = (sort: Parameters<typeof sortGames>[1]) => sortGames(all, sort).map((g) => g.title);

  it("sorts by title, recency, year and plays", () => {
    expect(titles("title")).toEqual(["Metroid", "Pokémon Puzzle", "Super Mario Bros.", "Tetris"]);
    expect(titles("recent")).toEqual(["Super Mario Bros.", "Metroid", "Pokémon Puzzle", "Tetris"]);
    expect(titles("year")).toEqual(["Super Mario Bros.", "Metroid", "Tetris", "Pokémon Puzzle"]);
    expect(titles("plays")).toEqual(["Metroid", "Super Mario Bros.", "Pokémon Puzzle", "Tetris"]);
  });

  it("sorts numbers naturally", () => {
    const list = sortGames([game("Mega Man 10"), game("Mega Man 2")], "title");
    expect(list.map((g) => g.title)).toEqual(["Mega Man 2", "Mega Man 10"]);
  });
});

describe("the A–Z strip", () => {
  const sorted = sortGames([game("1942"), game("Contra"), game("Metroid"), game("Zelda II")], "title");

  it("files titles under a letter or #", () => {
    expect(letterOf("Élite")).toBe("E");
    expect(letterOf("1942")).toBe("#");
  });

  it("jumps to the letter, else the next one that has a game", () => {
    expect(jumpTo(sorted, "#")).toBe(0);
    expect(jumpTo(sorted, "C")).toBe(1);
    expect(jumpTo(sorted, "D")).toBe(2);
    expect(jumpTo(sorted, "Z")).toBe(3);
    expect(jumpTo([], "A")).toBe(-1);
  });

  it("counts genres for the filters, splitting a game's list", () => {
    expect(genreCounts(all)).toEqual([
      { genre: "Action", count: 2 },
      { genre: "Platform", count: 1 },
      { genre: "Puzzle", count: 1 },
    ]);
  });
});

describe("the grid", () => {
  // 10 games, 4 a row: rows 0-3, 4-7, 8-9.
  it("moves along rows and between them", () => {
    expect(gridMove(0, "right", 4, 10)).toBe(1);
    expect(gridMove(3, "right", 4, 10)).toBe(4);
    expect(gridMove(4, "left", 4, 10)).toBe(3);
    expect(gridMove(1, "down", 4, 10)).toBe(5);
    expect(gridMove(5, "up", 4, 10)).toBe(1);
  });

  it("stops at the ends, and down from the last full row lands on the last game", () => {
    expect(gridMove(0, "left", 4, 10)).toBe(0);
    expect(gridMove(1, "up", 4, 10)).toBe(0);
    expect(gridMove(9, "right", 4, 10)).toBe(9);
    expect(gridMove(7, "down", 4, 10)).toBe(9);
    expect(gridMove(2, "pageDown", 4, 10)).toBe(9);
    expect(gridMove(0, "accept", 4, 10)).toBe(0);
    expect(gridMove(0, "right", 4, 0)).toBe(-1);
  });
});

describe("the jump strip", () => {
  const stops = jumpStops([game("1942"), game("Contra"), game("Castlevania"), game("Metroid"), game("Zelda")]);

  it("offers only letters that have games", () => {
    expect(stops).toEqual(["#", "C", "M", "Z"]);
  });

  it("shows the current letter with its neighbours, and steps without wrapping", () => {
    expect(stripWindow(stops, 1, 1)).toEqual({ before: ["#"], current: "C", after: ["M"] });
    expect(stepStop(stops, 3, 1)).toBe(3);
    expect(stepStop(stops, 0, -1)).toBe(0);
  });
});

describe("the panel's URL", () => {
  it("reads and writes ?game= and keeps the rest", () => {
    expect(gameFromUrl("?id=nes&game=42")).toBe(42);
    expect(gameFromUrl("?id=nes")).toBeNull();
    expect(gameFromUrl("?game=abc")).toBeNull();
    expect(urlWithGame("http://x/system/?id=nes", 42)).toBe("/system/?id=nes&game=42");
    expect(urlWithGame("http://x/system/?id=nes&game=42", null)).toBe("/system/?id=nes");
    expect(urlWithGame("http://x/system/?id=nes&game=1", 7)).toBe("/system/?id=nes&game=7");
  });
});

describe("favourites", () => {
  it("filters to them and counts the filter", () => {
    const fav = game("Liked", { favourite: true });
    const other = game("Other");
    expect(filterGames([fav, other], "", { favourites: true })).toEqual([fav]);
    expect(activeFilters({ favourites: true })).toBe(1);
  });
});
