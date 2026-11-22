import { describe, expect, it } from "vitest";
import type { AdminPatch } from "./api";
import { groupPatches, patchCount } from "./patches";

const patch = (over: Partial<AdminPatch>): AdminPatch => ({
  id: 1,
  name: "IPS patch",
  file: "nes/Game.ips",
  format: "IPS",
  isDefault: true,
  gameId: 10,
  gameTitle: "Game",
  system: "nes",
  chosenBy: 0,
  originalBy: 0,
  ...over,
});

describe("the patches page", () => {
  it("groups patches by system, then game, in the server's order", () => {
    const list = [
      patch({ id: 1 }),
      patch({ id: 2, isDefault: false, name: "Hard Mode" }),
      patch({ id: 3, gameId: 11, gameTitle: "Other" }),
      patch({ id: 4, system: "snes", gameId: 20, gameTitle: "Mother 2", originalBy: 2 }),
    ];
    const grouped = groupPatches(list);
    expect(grouped.map((s) => s.system)).toEqual(["nes", "snes"]);
    expect(grouped[0].games.map((g) => [g.title, g.patches.map((p) => p.id)])).toEqual([
      ["Game", [1, 2]],
      ["Other", [3]],
    ]);
    expect(grouped[1].games[0].originalBy).toBe(2);
    expect(groupPatches([])).toEqual([]);
  });

  it("counts patches and games", () => {
    expect(patchCount([patch({})])).toBe("1 patch for 1 game");
    expect(patchCount([patch({}), patch({ id: 2 }), patch({ id: 3, gameId: 11 })])).toBe("3 patches for 2 games");
  });
});
