import { describe, expect, it } from "vitest";
import { parseAspect, shouldFrame, systemDecorationEnv, windowInsets } from "./decoration";

describe("decorations", () => {
  it("reads the player's aspect ratio", () => {
    expect(parseAspect("4 / 3")).toBeCloseTo(4 / 3);
    expect(parseAspect("10/9")).toBeCloseTo(10 / 9);
    expect(parseAspect("nonsense")).toBeCloseTo(4 / 3);
  });

  it("frames the picture only when the screen is wider than it", () => {
    expect(shouldFrame(1920, 1080, 4 / 3)).toBe(true);
    expect(shouldFrame(1080, 1920, 4 / 3)).toBe(false);
    expect(shouldFrame(1440, 1080, 4 / 3)).toBe(false);
    expect(shouldFrame(1920, 1080, 16 / 9)).toBe(false);
  });

  it("places the window in percentages of the image", () => {
    const d = { url: "", width: 1920, height: 1080, top: 0, left: 240, bottom: 54, right: 240 };
    expect(windowInsets(d)).toEqual({ top: "0.0000%", left: "12.5000%", bottom: "5.0000%", right: "12.5000%" });
  });
});

describe("decoration settings", () => {
  it("are written as the line that sets them", () => {
    expect(systemDecorationEnv("snes", "arcade")).toBe("SOVEMU_SYSTEMS__snes__DECORATION=arcade");
  });
});
