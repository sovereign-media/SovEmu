import { describe, expect, it } from "vitest";
import { inAppPath } from "./links";

const origin = "https://sovemu.test";
const here = `${origin}/system/?id=snes`;

describe("in-app links", () => {
  it("routes links between screens, query and all", () => {
    expect(inAppPath("/bios/?id=psx", origin, here)).toBe("/bios/?id=psx");
    expect(inAppPath(`${origin}/`, origin, here)).toBe("/");
    expect(inAppPath("/admin/settings/#video", origin, here)).toBe("/admin/settings/#video");
  });

  it("leaves the server's own paths, other sites and in-page anchors to the browser", () => {
    expect(inAppPath("/media/games/7/thumb?Sov-Signature=x", origin, here)).toBeNull();
    expect(inAppPath("/oauth/authorize", origin, here)).toBeNull();
    expect(inAppPath("https://github.com/x", origin, here)).toBeNull();
    expect(inAppPath("#controls", origin, here)).toBeNull();
  });
});
