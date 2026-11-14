import { describe, expect, it } from "vitest";
import { DEFAULT_BINDINGS, bind, describeBinding, loadBindings } from "./input";

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
