import { describe, expect, it } from "vitest";
import { buildText, shortCommit } from "./about";

describe("the about page", () => {
  it("names the build", () => {
    const local = { version: "0.1.0", revision: null, built: null, libretroRelease: "1.22.2" };
    expect(buildText(local)).toBe("0.1.0 · local build");
    expect(
      buildText({ ...local, revision: "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678", built: "2026-11-22T13:09:37Z" }),
    ).toBe("0.1.0 · a1b2c3d · built 22 Nov 2026");
  });

  it("shortens commits", () => {
    expect(shortCommit("0863ee3700978e826b0d95b72d4742e537a7325b")).toBe("0863ee370097");
  });
});
