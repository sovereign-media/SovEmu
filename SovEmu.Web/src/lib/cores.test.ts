import { describe, expect, it } from "vitest";
import type { CoreReport } from "./api";
import { coreState, coresSummary, filterCores } from "./cores";

const core = (over: Partial<CoreReport>): CoreReport => ({
  name: "snes9x",
  label: "Snes9x",
  version: "1.63",
  file: "/opt/sovemu/cores/snes9x_libretro.so",
  loaded: true,
  error: null,
  licence: "Non-commercial",
  nonCommercial: true,
  sourceUrl: "https://github.com/libretro/snes9x",
  systems: [{ id: "snes", name: "Super Nintendo", chosen: true }],
  ...over,
});

describe("the cores page", () => {
  it("says whether a core loads", () => {
    expect(coreState(core({}))).toBe("loaded");
    expect(coreState(core({ loaded: false, error: "no libGL" }))).toBe("failed");
    expect(coreState(core({ loaded: false, file: null }))).toBe("missing");
  });

  it("finds cores by name, licence or system", () => {
    const all = [
      core({}),
      core({ name: "mgba", label: "mGBA", licence: "MPLv2.0", nonCommercial: false, systems: [] }),
    ];
    expect(filterCores(all, "").length).toBe(2);
    expect(filterCores(all, "super").map((c) => c.name)).toEqual(["snes9x"]);
    expect(filterCores(all, "mpl").map((c) => c.name)).toEqual(["mgba"]);
    expect(filterCores(all, "snes non-commercial").map((c) => c.name)).toEqual(["snes9x"]);
  });

  it("sums them up", () => {
    expect(coresSummary([core({}), core({ loaded: false, nonCommercial: false })], "1.22.2")).toBe(
      "2 cores from libretro 1.22.2 · 1 load · 1 non-commercial",
    );
  });
});
