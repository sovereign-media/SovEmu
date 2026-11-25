// The home screen: a hero of featured games over rows (continue playing, systems, recently
// added), as Sovereign's home. Moving between them with a pad or the keyboard, and which
// picture each game shows, are worked out here.
import type { Game, RecentGame } from "./api";
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
  row: Math.max(
    0,
    rows.findIndex((r) => r.length > 0),
  ),
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
    cols[focus.row] = shape.wraps
      ? (next + shape.length) % shape.length
      : Math.min(Math.max(next, 0), shape.length - 1);
  }
  return { row: focus.row, cols };
}

/** How far one click of a row's arrow moves it: most of the visible width, keeping a card of
 *  context. */
export const SCROLL_STEP = 0.8;

/** Where an arrow click takes a row scrolled to `left` (of `total`, `width` visible): a step
 *  that way, and from either end round to the other, so a row loops as the hero does. */
export function nextScroll(left: number, width: number, total: number, dir: -1 | 1): number {
  const max = Math.max(0, total - width);
  const atEnd = dir > 0 ? left >= max - 4 : left <= 4;
  if (atEnd) return dir > 0 ? 0 : max;
  return Math.min(max, Math.max(0, left + dir * width * SCROLL_STEP));
}

/** The hero's backdrop: a screenshot-style image if there is one, else the box art (large). */
export function heroArt(g: Pick<Game, "image" | "cover" | "thumb">): string | null {
  return g.image ?? g.cover ?? g.thumb ?? null;
}

/** A card's picture: where the player left off (the auto save's screenshot), else artwork at a
 *  card's size. */
export function cardArt(
  g: Pick<Game, "imageSmall" | "thumb"> & { autoSave?: RecentGame["autoSave"] },
): { url: string; kind: "screenshot" | "art" } | null {
  const shot = g.autoSave?.screenshotUrl;
  if (shot) return { url: shot, kind: "screenshot" };
  const art = g.imageSmall ?? g.thumb ?? null;
  return art ? { url: art, kind: "art" } : null;
}

/** "1996 · Nintendo · Platform", from what is known. */
export function byline(g: Game): string {
  return [g.releaseDate?.slice(0, 4), g.developer ?? g.publisher, g.genre].filter(Boolean).join(" · ");
}

/** Home's Continue panel's buttons, left to right: Continue (or Play), then Game details. */
export type ResumeButton = "play" | "details";

/** What a pad or key does while the Continue panel is open: move between its two buttons,
 *  press the one focused, or close it (Back). Anything else does nothing: the rows behind it
 *  stay where they are. */
export function resumeNav(
  at: ResumeButton,
  action: NavAction,
): { at: ResumeButton } | { press: ResumeButton } | "close" | null {
  if (action === "left") return { at: "play" };
  if (action === "right") return { at: "details" };
  if (action === "accept") return { press: at };
  if (action === "back") return "close";
  return null;
}
