import { describe, expect, it } from "vitest";
import type { Game } from "./api";
import { byline, cardArt, heroArt, moveFocus, nextScroll, type RowShape, resumeNav, startFocus } from "./home";

const game = (over: Partial<Game> = {}): Game => ({
  id: 7,
  system: "snes",
  title: "A Game",
  tags: [],
  path: "a.sfc",
  size: 1,
  lastPlayedAt: null,
  playCount: 0,
  image: null,
  imageSmall: null,
  cover: null,
  thumb: null,
  video: null,
  fileTitle: null,
  description: null,
  releaseDate: null,
  developer: null,
  publisher: null,
  genre: null,
  players: null,
  rating: null,
  esrb: null,
  modes: [],
  series: [],
  themes: [],
  favourite: false,
  release: null,
  patch: null,
  discs: 1,
  partKind: null,
  ...over,
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
    expect(heroArt(game({ thumb: "/media/games/7/thumb", image: "/media/games/7/image" }))).toBe(
      "/media/games/7/image",
    );
    expect(heroArt(game({ thumb: "/media/games/7/thumb" }))).toBe("/media/games/7/thumb");
    expect(heroArt(game({ thumb: "/t400", cover: "/t800" }))).toBe("/t800"); // the larger box art
    expect(heroArt(game())).toBeNull();
  });

  it("shows where the player left off on a recent game", () => {
    const autoSave = { slot: 0, savedAt: 1700, screenshotUrl: "/media/games/7/saves/0?t=1700" };
    expect(cardArt({ ...game({ image: "/media/games/7/image" }), autoSave })).toEqual({
      url: "/media/games/7/saves/0?t=1700",
      kind: "screenshot",
    });
    expect(cardArt({ ...game({ thumb: "/t" }), autoSave: { ...autoSave, screenshotUrl: null } })?.kind).toBe("art");
    expect(cardArt(game({ thumb: "/t" }))?.kind).toBe("art");
    // A card asks for artwork at a card's size, not the backdrop's.
    expect(cardArt(game({ image: "/i1600", imageSmall: "/i480", thumb: "/t" }))?.url).toBe("/i480");
    expect(cardArt(game())).toBeNull();
  });

  it("builds a byline from what is known", () => {
    expect(byline(game({ releaseDate: "1996-06-23", developer: "Nintendo", genre: "Platform" }))).toBe(
      "1996 · Nintendo · Platform",
    );
    expect(byline(game({ publisher: "Capcom" }))).toBe("Capcom");
    expect(byline(game())).toBe("");
  });
});

describe("row arrows", () => {
  // 1000 px of cards, 400 px visible: the row scrolls from 0 to 600.
  it("steps most of a screen, stopping at the ends", () => {
    expect(nextScroll(0, 400, 1000, 1)).toBe(320);
    expect(nextScroll(500, 400, 1000, 1)).toBe(600);
    expect(nextScroll(100, 400, 1000, -1)).toBe(0);
  });

  it("loops from either end to the other", () => {
    expect(nextScroll(600, 400, 1000, 1)).toBe(0);
    expect(nextScroll(0, 400, 1000, -1)).toBe(600);
    expect(nextScroll(598, 400, 1000, 1)).toBe(0);
  });

  it("stays put when the row fits", () => {
    expect(nextScroll(0, 400, 300, 1)).toBe(0);
    expect(nextScroll(0, 400, 300, -1)).toBe(0);
  });
});

describe("resumeNav", () => {
  it("moves between Continue and Game details, staying at either end", () => {
    expect(resumeNav("play", "right")).toEqual({ at: "details" });
    expect(resumeNav("details", "right")).toEqual({ at: "details" });
    expect(resumeNav("details", "left")).toEqual({ at: "play" });
    expect(resumeNav("play", "left")).toEqual({ at: "play" });
  });

  it("presses the focused button, and Back closes the panel", () => {
    expect(resumeNav("play", "accept")).toEqual({ press: "play" });
    expect(resumeNav("details", "accept")).toEqual({ press: "details" });
    expect(resumeNav("details", "back")).toBe("close");
  });

  it("keeps the rows behind it still", () => {
    for (const action of ["up", "down", "menu", "search", "pageUp", "pageDown"] as const) {
      expect(resumeNav("play", action)).toBeNull();
    }
  });
});
