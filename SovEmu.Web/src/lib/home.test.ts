import { describe, expect, it } from "vitest";
import type { Game } from "./api";
import { byline, cardArt, heroArt, moveFocus, startFocus, type RowShape } from "./home";

const game = (over: Partial<Game> = {}): Game => ({
  id: 7, system: "snes", title: "A Game", tags: "", path: "a.sfc", size: 1, last_played_at: null, play_count: null,
  media: [], file_title: null, description: null, release_date: null, developer: null, publisher: null, genre: null,
  players: null, rating: null, ...over,
});

describe("home focus", () => {
  // Hero (wraps), continue playing (empty), systems, recently added.
  const rows: RowShape[] = [{ length: 3, wraps: true }, { length: 0 }, { length: 4 }, { length: 2 }];

  it("starts on the first row with something in it", () => {
    expect(startFocus(rows).row).toBe(0);
    expect(startFocus([{ length: 0 }, { length: 2 }]).row).toBe(1);
  });

  it("skips empty rows and stops at the ends", () => {
    let f = startFocus(rows);
    f = moveFocus(f, "down", rows);
    expect(f.row).toBe(2);
    f = moveFocus(f, "down", rows);
    f = moveFocus(f, "down", rows);
    expect(f.row).toBe(3);
    f = moveFocus(f, "up", rows);
    expect(f.row).toBe(2);
    expect(moveFocus(startFocus(rows), "up", rows).row).toBe(0);
  });

  it("wraps the hero, clamps the rows and remembers each row's column", () => {
    let f = startFocus(rows);
    f = moveFocus(f, "left", rows);
    expect(f.cols[0]).toBe(2);
    f = moveFocus(f, "down", rows);
    for (let i = 0; i < 6; i++) f = moveFocus(f, "right", rows);
    expect(f.cols[2]).toBe(3);
    f = moveFocus(moveFocus(f, "up", rows), "down", rows);
    expect(f.cols[2]).toBe(3);
    expect(f.cols[0]).toBe(2);
  });

  it("keeps a remembered column inside a row that shrank", () => {
    const f = { row: 2, cols: [0, 0, 9, 0] };
    expect(moveFocus(f, "accept", rows).cols[2]).toBe(3);
  });
});

describe("home art", () => {
  it("prefers the image, then box art", () => {
    expect(heroArt(game({ media: ["thumb", "image"] }))).toBe("/api/games/7/media/image");
    expect(heroArt(game({ media: ["thumb"] }))).toBe("/api/games/7/media/thumb");
    expect(heroArt(game())).toBeNull();
  });

  it("shows where the player left off on a recent game", () => {
    expect(cardArt({ ...game({ media: ["image"] }), screenshot_at: 1700 })).toEqual({
      url: "/api/games/7/states/0/screenshot?t=1700",
      kind: "screenshot",
    });
    expect(cardArt(game({ media: ["thumb"] }))?.kind).toBe("art");
    expect(cardArt(game())).toBeNull();
  });

  it("builds a byline from what is known", () => {
    expect(byline(game({ release_date: "1996-06-23", developer: "Nintendo", genre: "Platform" }))).toBe("1996 · Nintendo · Platform");
    expect(byline(game({ publisher: "Capcom" }))).toBe("Capcom");
    expect(byline(game())).toBe("");
  });
});
