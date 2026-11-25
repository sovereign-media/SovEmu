// How a game's box art sits in its card. A system's cards take the shape of its boxes
// (`System.boxAspect`: SNES landscape, NES portrait …); art close to that shape fills the card,
// and art of another shape (a Super Famicom's portrait box in a SNES grid, a screenshot) is shown
// whole inside it rather than cut. No React, so Vitest can check it.

/** How far (as a ratio) art's shape may be from its card's and still fill it. */
const CLOSE = 1.15;

/** The card's shape when its system gives none: the old portrait card. */
export const DEFAULT_ASPECT = 3 / 4;

/** `cover` to fill the card (trimming a little), `contain` to show the art whole inside it. */
export function artFit(imageAspect: number, cardAspect: number): "cover" | "contain" {
  if (!(imageAspect > 0) || !(cardAspect > 0)) return "cover";
  return Math.abs(Math.log(imageAspect / cardAspect)) <= Math.log(CLOSE) ? "cover" : "contain";
}
