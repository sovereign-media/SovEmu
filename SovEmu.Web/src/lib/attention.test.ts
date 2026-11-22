import { describe, expect, it } from "vitest";
import type { AttentionItem } from "./api";
import { canLookUp, detailText, itemName, kindFrom, tally } from "./attention";

const item = (over: Partial<AttentionItem>): AttentionItem => ({
  gameId: 1,
  system: "psx",
  title: "Big",
  path: "psx/Big (Disc 1).chd",
  detail: "",
  ...over,
});

describe("attention", () => {
  it("sums each kind and lists one kind per system", () => {
    const { totals, systems } = tally(
      [
        { kind: "DUPLICATE", system: "gba", count: 2 },
        { kind: "DUPLICATE", system: "nes", count: 3 },
        { kind: "NO_BOX_ART", system: "gba", count: 1 },
      ],
      "DUPLICATE",
    );
    expect(totals).toEqual({ NO_INFORMATION: 0, NO_BOX_ART: 1, DUPLICATE: 5, MISSING_PARTS: 0, UNRECOGNISED: 0 });
    expect(systems).toEqual([
      { system: "gba", count: 2 },
      { system: "nes", count: 3 },
    ]);
  });

  it("names a game by its title and a file by its name", () => {
    expect(itemName(item({}))).toBe("Big");
    expect(itemName(item({ gameId: null, title: "", path: "nes/notes.abc" }))).toBe("notes.abc");
  });

  it("says each kind's detail in words", () => {
    expect(detailText("NO_INFORMATION", item({ detail: "not found" }))).toBe("Looked up, not found");
    expect(detailText("NO_INFORMATION", item({ detail: "never looked up" }))).toBe("Never looked up");
    expect(detailText("DUPLICATE", item({ detail: "USA" }))).toBe("Tags: USA");
    expect(detailText("DUPLICATE", item({}))).toBe("No tags");
    expect(detailText("MISSING_PARTS", item({ detail: "disc 2" }))).toBe("Missing disc 2");
  });

  it("offers a lookup only where one can help, and reads the kind from the URL", () => {
    expect(canLookUp("NO_BOX_ART")).toBe(true);
    expect(canLookUp("DUPLICATE")).toBe(false);
    expect(kindFrom("MISSING_PARTS")).toBe("MISSING_PARTS");
    expect(kindFrom("nonsense")).toBe("NO_INFORMATION");
  });
});
