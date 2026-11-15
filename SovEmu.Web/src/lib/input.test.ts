import { describe, expect, it } from "vitest";
import { DEFAULT_PAD, rebind } from "./controllers";
import { axisToStick, padState, triggerValue } from "./input";
import { bit } from "./protocol";

describe("a gamepad through its mapping", () => {
  it("reads standard buttons by position", () => {
    const held = new Set([0, 12]); // south and D-pad up
    expect(padState((i) => held.has(i), DEFAULT_PAD)).toBe(bit("B") | bit("UP"));
  });

  it("follows a remap", () => {
    const swapped = rebind(rebind(DEFAULT_PAD, "east", "B"), "south", "A");
    expect(padState((i) => i === 0, swapped)).toBe(bit("A"));
    expect(padState((i) => i === 1, swapped)).toBe(bit("B"));
  });
});

describe("analog", () => {
  it("takes out the dead zone and scales to libretro's range", () => {
    expect(axisToStick(0.1)).toBe(0);
    expect(axisToStick(-0.1)).toBe(0);
    expect(axisToStick(1)).toBe(32767);
    expect(axisToStick(-1)).toBe(-32767);
    expect(Math.abs(axisToStick(0.575) - 16384)).toBeLessThanOrEqual(1); // half travel
    expect(axisToStick(1.2)).toBe(32767);
  });

  it("scales triggers", () => {
    expect(triggerValue(0)).toBe(0);
    expect(triggerValue(1)).toBe(32767);
    expect(triggerValue(-0.5)).toBe(0);
  });
});
