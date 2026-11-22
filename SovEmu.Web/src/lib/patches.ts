// The admin Patches page's grouping: patches by system, then by game. No React, so Vitest can
// check it.

import type { AdminPatch } from "./api";

export interface PatchedGame {
  id: number;
  title: string;
  system: string;
  /** Players who chose the original over the default. */
  originalBy: number;
  /** The default first, as the server orders them. */
  patches: AdminPatch[];
}

export interface PatchedSystem {
  system: string;
  games: PatchedGame[];
}

/** Groups the server's list (ordered by system, game, then default first), keeping its order. */
export function groupPatches(patches: AdminPatch[]): PatchedSystem[] {
  const systems: PatchedSystem[] = [];
  for (const p of patches) {
    let system = systems.at(-1);
    if (system?.system !== p.system) {
      system = { system: p.system, games: [] };
      systems.push(system);
    }
    let game = system.games.at(-1);
    if (game?.id !== p.gameId) {
      game = { id: p.gameId, title: p.gameTitle, system: p.system, originalBy: p.originalBy, patches: [] };
      system.games.push(game);
    }
    game.patches.push(p);
  }
  return systems;
}

/** "3 patches for 2 games". */
export function patchCount(patches: AdminPatch[]): string {
  const games = new Set(patches.map((p) => p.gameId)).size;
  const patchWord = patches.length === 1 ? "patch" : "patches";
  const gameWord = games === 1 ? "game" : "games";
  return `${patches.length} ${patchWord} for ${games} ${gameWord}`;
}
