import { describe, expect, it } from "vitest";
import { DEFAULT_PAD, rebind } from "./controllers";
import { axisToStick, InputState, padState, touchControlsWanted, triggerValue } from "./input";
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

describe("the on-screen controls", () => {
  it("show only on a touch screen with nothing else to play with", () => {
    // The Android app's rule: a phone with a pad or a keyboard plays with it, and buttons drawn
    // over the game would only hide the picture. A desktop never shows them.
    expect(touchControlsWanted(true, 0, false)).toBe(true);
    expect(touchControlsWanted(true, 1, false)).toBe(false);
    expect(touchControlsWanted(true, 0, true)).toBe(false);
    expect(touchControlsWanted(false, 0, false)).toBe(false);
  });

  it("drive player 1, and a button let go is no longer held", () => {
    // A button that stayed set after the finger lifted would hold it in the game for good.
    const input = new InputState(new EventTarget() as unknown as Window);
    input.touch("A", true);
    input.touch("LEFT", true);
    expect(input.read().ports[0]).toBe(bit("A") | bit("LEFT"));
    input.touch("A", false);
    expect(input.read().ports[0]).toBe(bit("LEFT"));
    input.dispose();
  });

  it("send a tap shorter than a frame once", () => {
    // Input is read once per animation frame: a press and release between two reads would
    // otherwise never be sent, and a quick tap on Start would do nothing.
    const input = new InputState(new EventTarget() as unknown as Window);
    input.touch("START", true);
    input.touch("START", false);
    expect(input.read().ports[0]).toBe(bit("START"));
    expect(input.read().ports[0]).toBe(0);
    input.dispose();
  });
});
