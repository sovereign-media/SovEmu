import { describe, expect, it } from "vitest";
import type { Game } from "./api";
import { activeFilters, filterGames, jumpTo, letterOf, maxPlayers, sortGames, tagCounts } from "./games";

let nextId = 1;
const game = (title: string, over: Partial<Game> = {}): Game => ({
  id: nextId++, system: "nes", title, tags: "", path: `${title}.nes`, size: 1, last_played_at: null, play_count: null,
  media: [], file_title: null, description: null, release_date: null, developer: null, publisher: null, genre: null,
  players: null, rating: null, ...over,
});

const metroid = game("Metroid", { release_date: "1986-08-06", genre: "Action", players: "1", play_count: 3, last_played_at: "2026-11-13 10:00:00", tags: "USA" });
const mario = game("Super Mario Bros.", { release_date: "1985-09-13", genre: "Platform", players: "1-2", play_count: 1, last_played_at: "2026-11-14 09:00:00", tags: "USA, Europe" });
const pokemon = game("Pokémon Puzzle", { players: "2", tags: "Europe" });
const tetris = game("Tetris", { release_date: "1989" });
const all = [tetris, pokemon, mario, metroid];

describe("searching and filtering", () => {
  it("matches every word, ignoring case and accents", () => {
    expect(filterGames(all, "mario", {}).map((g) => g.title)).toEqual(["Super Mario Bros."]);
    expect(filterGames(all, "POKEMON", {})).toHaveLength(1);
    expect(filterGames(all, "action metroid", {})).toHaveLength(1);
    expect(filterGames(all, "europe", {}).map((g) => g.title)).toEqual(["Pokémon Puzzle", "Super Mario Bros."]);
    expect(filterGames(all, "", {})).toHaveLength(4);
  });

  it("filters by played, players and tags", () => {
    expect(filterGames(all, "", { played: "played" }).map((g) => g.title).sort()).toEqual(["Metroid", "Super Mario Bros."]);
    expect(filterGames(all, "", { played: "unplayed" })).toHaveLength(2);
    expect(filterGames(all, "", { multiplayer: true }).map((g) => g.title).sort()).toEqual(["Pokémon Puzzle", "Super Mario Bros."]);
    expect(filterGames(all, "", { tags: ["USA", "Europe"] }).map((g) => g.title)).toEqual(["Super Mario Bros."]);
    expect(activeFilters({ played: "played", multiplayer: true, tags: ["USA"] })).toBe(3);
  });

  it("reads the most players from the metadata's wording", () => {
    expect(maxPlayers("1-2")).toBe(2);
    expect(maxPlayers("1 to 4")).toBe(4);
    expect(maxPlayers(null)).toBeNull();
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

  it("counts tags for the filter menu", () => {
    expect(tagCounts(all)).toEqual([
      { tag: "Europe", count: 2 },
      { tag: "USA", count: 2 },
    ]);
  });
});
