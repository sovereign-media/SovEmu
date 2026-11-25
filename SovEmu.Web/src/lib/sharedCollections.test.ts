import { describe, expect, it } from "vitest";
import type { Collection, CollectionGame } from "./api";
import { environmentLines } from "./sharedCollections";

const collection = (name: string, games: Pick<CollectionGame, "system" | "path">[]) =>
  ({ id: 1, name, gameCount: games.length, shared: true, declared: false, games }) as unknown as Collection;

describe("shared collections", () => {
  it("writes each collection and its games as environment lines, in order", () => {
    expect(
      environmentLines([
        collection("Family night", [
          { system: "nes", path: "nes/Super Stand-in (USA).nes" },
          { system: "psx", path: "psx/Big/Big (Disc 1).chd" },
        ]),
        collection("Empty", []),
      ]),
    ).toEqual([
      "SovEmu__Collections__0__Name=Family night",
      "SovEmu__Collections__0__Games__0=nes/Super Stand-in (USA).nes",
      "SovEmu__Collections__0__Games__1=psx/Big (Disc 1).chd",
      "SovEmu__Collections__1__Name=Empty",
    ]);
  });
});
