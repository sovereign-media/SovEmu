// The home screen: a hero of featured games over rows (continue playing, systems, recently
// added), as Sovereign's home. Moving between them with a pad or the keyboard, and which
// picture each game shows, are worked out here.
import { mediaUrl, type Game } from "./api";
import type { NavAction } from "./nav";

/** Where the focus is: a row, and the column last focused in every row. */
export interface Focus {
  row: number;
  cols: number[];
}

/** Rows that wrap at their ends (the hero) rather than stopping. */
export interface RowShape {
  length: number;
  wraps?: boolean;
}

export const startFocus = (rows: RowShape[]): Focus => ({
  row: Math.max(0, rows.findIndex((r) => r.length > 0)),
  cols: rows.map(() => 0),
});

/** The focus after `action`; up and down skip empty rows, left and right stay in the row. */
export function moveFocus(focus: Focus, action: NavAction, rows: RowShape[]): Focus {
  const cols = rows.map((r, i) => Math.min(focus.cols[i] ?? 0, Math.max(0, r.length - 1)));
  const shape = rows[focus.row];
  if (action === "up" || action === "down") {
    const dir = action === "up" ? -1 : 1;
    for (let r = focus.row + dir; r >= 0 && r < rows.length; r += dir) {
      if (rows[r].length > 0) return { row: r, cols };
    }
    return { row: focus.row, cols };
  }
  if ((action === "left" || action === "right") && shape?.length) {
    const dir = action === "left" ? -1 : 1;
    const next = cols[focus.row] + dir;
    cols[focus.row] = shape.wraps ? (next + shape.length) % shape.length : Math.min(Math.max(next, 0), shape.length - 1);
  }
  return { row: focus.row, cols };
}

/** The hero's backdrop: a screenshot-style image if there is one, else the box art. */
export function heroArt(g: Game): string | null {
  if (g.media.includes("image")) return mediaUrl(g.id, "image");
  if (g.media.includes("thumb")) return mediaUrl(g.id, "thumb");
  return null;
}

/** A card's picture: where the player left off (the auto save's screenshot), else artwork. */
export function cardArt(g: Game & { screenshot_at?: number | null }): { url: string; kind: "screenshot" | "art" } | null {
  if (g.screenshot_at) return { url: `/api/games/${g.id}/states/0/screenshot?t=${g.screenshot_at}`, kind: "screenshot" };
  const art = heroArt(g);
  return art ? { url: art, kind: "art" } : null;
}

/** "1996 · Nintendo · Platform", from what is known. */
export function byline(g: Game): string {
  return [g.release_date?.slice(0, 4), g.developer ?? g.publisher, g.genre].filter(Boolean).join(" · ");
}
