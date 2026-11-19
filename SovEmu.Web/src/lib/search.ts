// Search across every system: where the pad or keyboard focus goes on the search page. The focus
// is the search field (FIELD) or a result, by index, in a grid `columns` wide.
import type { NavAction } from "./nav";

export const FIELD = -1;

/** How long typing must pause before the server is asked. */
export const SEARCH_DELAY_MS = 200;

/** How many results the server sends by default (`Query.search`). */
export const SEARCH_LIMIT = 48;

/** The focus after `action`: down from the field enters the grid, up from its top row leaves it. */
export function moveSearch(at: number, action: NavAction, count: number, columns: number): number {
  const cols = Math.max(1, columns);
  if (count === 0) return FIELD;
  if (at === FIELD) return action === "down" ? 0 : FIELD;
  switch (action) {
    case "up":
      return at < cols ? FIELD : at - cols;
    case "down":
      // Down from a row above a short last row lands on its last card, as a grid does.
      return at + cols < count ? at + cols : Math.floor(at / cols) < Math.floor((count - 1) / cols) ? count - 1 : at;
    case "left":
      return at % cols === 0 ? at : at - 1;
    case "right":
      return at % cols === cols - 1 || at === count - 1 ? at : at + 1;
    case "pageUp":
      return Math.max(0, at - cols * 3);
    case "pageDown":
      return Math.min(count - 1, at + cols * 3);
    default:
      return at;
  }
}
