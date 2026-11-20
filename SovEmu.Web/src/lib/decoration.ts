// Decorations (bezels) around the game's picture: artwork from the server (`Game.decoration`,
// Batocera's format) with a transparent window the picture shows through. The image is fitted to
// the screen and the picture, keeping its shape, to the scaled window.

export interface Decoration {
  url: string;
  width: number;
  height: number;
  top: number;
  left: number;
  bottom: number;
  right: number;
}

/** "4 / 3" (the player's aspect ratio) as a number. */
export function parseAspect(aspect: string): number {
  const [w, h] = aspect.split("/").map((n) => Number(n.trim()));
  return w > 0 && h > 0 ? w / h : 4 / 3;
}

/**
 * Whether to frame the picture: only when the screen is wider than it, so there are bars to fill
 * (a phone held upright shows the picture alone, as large as it can be).
 */
export function shouldFrame(screenWidth: number, screenHeight: number, aspect: number): boolean {
  return screenHeight > 0 && screenWidth / screenHeight > aspect * 1.05;
}

/** The window, as percentages of the image, for absolutely positioning the picture in it. */
export function windowInsets(d: Decoration): { top: string; left: string; bottom: string; right: string } {
  const pct = (n: number, of: number) => `${((n / of) * 100).toFixed(4)}%`;
  return { top: pct(d.top, d.height), left: pct(d.left, d.width), bottom: pct(d.bottom, d.height), right: pct(d.right, d.width) };
}
