import { describe, expect, it } from "vitest";
import { ESRB, esrbLabel, stars } from "./esrb";

describe("ESRB ratings", () => {
  it("has a badge and a name for every rating", () => {
    for (const info of Object.values(ESRB)) {
      expect(info.badge).toBeTruthy();
      expect(info.name).toBeTruthy();
    }
    expect(Object.keys(ESRB)).toHaveLength(7);
  });

  it("labels a badge for screen readers", () => {
    expect(esrbLabel("EVERYONE_10_PLUS")).toBe("ESRB: Everyone 10+ (E10+)");
  });

  it("turns a rating into stars to the half", () => {
    expect(stars(0.8)).toBe(4);
    expect(stars(0.75)).toBe(4);
    expect(stars(0.72)).toBe(3.5);
    expect(stars(1.4)).toBe(5);
    expect(stars(-1)).toBe(0);
  });
});
