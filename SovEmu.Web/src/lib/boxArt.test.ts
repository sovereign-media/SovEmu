import { describe, expect, it } from "vitest";
import { artFit } from "./boxArt";

describe("box art in a card", () => {
  it("fills a card of nearly its own shape", () => {
    expect(artFit(1.4, 1.4)).toBe("cover");
    expect(artFit(1.33, 1.4)).toBe("cover"); // a US SNES box in a SNES card
    expect(artFit(0.7, 0.72)).toBe("cover");
  });

  it("shows art of another shape whole, uncut", () => {
    expect(artFit(0.72, 1.4)).toBe("contain"); // a Super Famicom box in a SNES card
    expect(artFit(1.4, 0.72)).toBe("contain");
    expect(artFit(4 / 3, 1.0)).toBe("contain"); // a screenshot in a jewel-case card
  });

  it("fills the card while the art's shape is unknown", () => {
    expect(artFit(0, 1.4)).toBe("cover");
    expect(artFit(Number.NaN, 1.4)).toBe("cover");
  });
});
