// The admin Shared collections page's environment lines. No React, so Vitest can check them.

import type { Collection } from "./api";

/**
 * The shared collections as `SovEmu__Collections__…` lines, each game as `<system id>/<file
 * name>` in the order the collection lists them (the newest added first), so that declaring them
 * keeps what the admin made. Every collection, declared ones included: the lines replace them all.
 */
export function environmentLines(collections: Collection[]): string[] {
  return collections.flatMap((c, n) => [
    `SovEmu__Collections__${n}__Name=${c.name}`,
    ...c.games.map((g, m) => `SovEmu__Collections__${n}__Games__${m}=${g.system}/${fileName(g.path)}`),
  ]);
}

/** A library path's last part. */
function fileName(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}
