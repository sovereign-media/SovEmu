import { describe, expect, it } from "vitest";
import type { SystemSummary } from "./api";
import { groupByMaker, initials, systemBadge } from "./systems";

const sys = (over: Partial<SystemSummary>): SystemSummary => ({
  id: "n64", name: "Nintendo 64", short: "N64", manufacturer: "Nintendo", year: 1996, color: "#000",
  game_count: 1, playable: true, needs_gpu: false, ...over,
});

describe("system badges", () => {
  it("says why a system cannot play, or that it plays slowly", () => {
    expect(systemBadge(sys({}))).toBeNull();
    expect(systemBadge(sys({ playable: false }))).toBe("no core");
    expect(systemBadge(sys({ needs_gpu: true, rendering: "gpu" }))).toBeNull();
    expect(systemBadge(sys({ needs_gpu: true, rendering: "software" }))).toBe("software rendering");
    expect(systemBadge(sys({ needs_gpu: true, rendering: "off", playable: false }))).toBe("no 3D rendering");
  });
});

describe("systems menu", () => {
  it("groups systems by maker in SovEmu's order", () => {
    const groups = groupByMaker([
      sys({ id: "nes", manufacturer: "Nintendo" }),
      sys({ id: "megadrive", manufacturer: "Sega" }),
      sys({ id: "snes", manufacturer: "Nintendo" }),
      sys({ id: "fbneo", manufacturer: "FinalBurn Neo" }),
    ]);
    expect(groups.map((g) => [g.maker, g.systems.map((s) => s.id)])).toEqual([
      ["Nintendo", ["nes", "snes"]],
      ["Sega", ["megadrive"]],
      ["FinalBurn Neo", ["fbneo"]],
    ]);
  });

  it("makes avatar initials", () => {
    expect(initials("Bruce Lee")).toBe("BL");
    expect(initials("mum")).toBe("M");
    expect(initials("dad_2")).toBe("D2");
    expect(initials("")).toBe("?");
  });
});
