// The graphics behind every screen: subtle, geometric, drawn to sit under grids and rows without
// competing with them. There are several, and each visit to the app gets one at random (the page
// does not reload between screens, so it stays put until the next visit). Each has an id so a
// user's choice, and their own image, can join the list later.
import dither from "../assets/backgrounds/dither.svg";
import lattice from "../assets/backgrounds/lattice.svg";
import screens from "../assets/backgrounds/screens.svg";

export interface Background {
  id: string;
  name: string;
  /** The image's URL: transparent, laid over the surface colour and scaled to cover the screen. */
  url: string;
}

export const BACKGROUNDS: Background[] = [
  { id: "dither", name: "Dither", url: dither },
  { id: "lattice", name: "Lattice", url: lattice },
  { id: "screens", name: "Screens", url: screens },
];

/** One of the backgrounds, by a random number in [0, 1). */
export function pickBackground(random: number = Math.random()): Background {
  const index = Math.min(BACKGROUNDS.length - 1, Math.max(0, Math.floor(random * BACKGROUNDS.length)));
  return BACKGROUNDS[index];
}

/** This visit's background. */
export const background = pickBackground();
