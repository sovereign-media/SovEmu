import { describe, expect, it } from "vitest";
import { type NavAction, REPEAT_DELAY_MS, REPEAT_INTERVAL_MS, Repeater } from "./nav";

const held = (...a: NavAction[]) => new Set<NavAction>(a);

describe("Repeater", () => {
  it("fires once on press, then repeats directions after a delay", () => {
    const r = new Repeater();
    expect(r.update(held("down"), 0)).toEqual(["down"]);
    expect(r.update(held("down"), 100)).toEqual([]);
    expect(r.update(held("down"), REPEAT_DELAY_MS)).toEqual(["down"]);
    expect(r.update(held("down"), REPEAT_DELAY_MS + 10)).toEqual([]);
    expect(r.update(held("down"), REPEAT_DELAY_MS + REPEAT_INTERVAL_MS)).toEqual(["down"]);
  });

  it("never repeats buttons", () => {
    const r = new Repeater();
    expect(r.update(held("accept"), 0)).toEqual(["accept"]);
    expect(r.update(held("accept"), 2000)).toEqual([]);
    expect(r.update(held(), 2001)).toEqual([]);
    expect(r.update(held("accept"), 2002)).toEqual(["accept"]);
  });
});
