import { describe, expect, it } from "vitest";
import { BACKGROUNDS, background, pickBackground } from "./backgrounds";

describe("backgrounds", () => {
  it("picks each background with equal chance, and never falls off the list", () => {
    expect(pickBackground(0).id).toBe("dither");
    expect(pickBackground(0.34).id).toBe("lattice");
    expect(pickBackground(0.99).id).toBe("screens");
    expect(pickBackground(1).id).toBe("screens");
    expect(pickBackground(-1).id).toBe("dither");
  });

  it("has an image for every background, and one chosen for this visit", () => {
    expect(new Set(BACKGROUNDS.map((b) => b.id)).size).toBe(BACKGROUNDS.length);
    for (const b of BACKGROUNDS) expect(b.url).toMatch(/\.svg|^data:image\/svg/);
    expect(BACKGROUNDS).toContain(background);
  });
});
