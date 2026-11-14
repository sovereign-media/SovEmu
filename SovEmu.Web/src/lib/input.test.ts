import { describe, expect, it } from "vitest";
import { DEFAULT_BINDINGS, axisToStick, bind, describeBinding, loadBindings, triggerValue } from "./input";

describe("bindings", () => {
  it("falls back to defaults when nothing (or garbage) is stored", () => {
    expect(loadBindings({ getItem: () => null })).toEqual(DEFAULT_BINDINGS);
    expect(loadBindings({ getItem: () => "{oops" })).toEqual(DEFAULT_BINDINGS);
  });

  it("rebinds without touching the defaults", () => {
    const b = bind(DEFAULT_BINDINGS, { key: "KeyJ" }, "A");
    expect(b.keys.KeyJ).toBe("A");
    expect(DEFAULT_BINDINGS.keys.KeyJ).toBeUndefined();
    const p = bind(b, { pad: 0 }, "A");
    expect(p.pad[0]).toBe("A");
    expect(describeBinding(p, "A")).toBe("X, J, pad 0, pad 1");
    expect(describeBinding(p, "L3")).toBe("pad 10");
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
