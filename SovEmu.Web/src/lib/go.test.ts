import { describe, expect, it } from "vitest";
import { pathOf, screenKey } from "./go";

describe("screen keys", () => {
  it("a screen's own parameters do not make it a new screen", () => {
    expect(screenKey("/system/?id=snes&game=7", ["game"])).toBe(screenKey("/system/?id=snes", ["game"]));
    expect(screenKey("/system/?id=snes", ["game"])).not.toBe(screenKey("/system/?id=nes", ["game"]));
    expect(screenKey("/play/?game=1&start=0")).not.toBe(screenKey("/play/?game=2&start=0"));
  });

  it("names a URL's screen by its path, trailing slash and all", () => {
    expect(pathOf("/system/?id=snes&game=7")).toBe("/system/");
    expect(pathOf("/play?game=1")).toBe("/play/");
    expect(pathOf("/")).toBe("/");
  });
});
