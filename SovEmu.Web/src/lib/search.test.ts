import { describe, expect, it } from "vitest";
import { FIELD, moveSearch } from "./search";

describe("moveSearch", () => {
  // 7 results, 3 columns:  0 1 2 / 3 4 5 / 6
  const move = (at: number, action: Parameters<typeof moveSearch>[1]) => moveSearch(at, action, 7, 3);

  it("enters the grid from the field and leaves it from the top row", () => {
    expect(move(FIELD, "down")).toBe(0);
    expect(move(FIELD, "right")).toBe(FIELD);
    expect(move(1, "up")).toBe(FIELD);
    expect(move(4, "up")).toBe(1);
  });

  it("stops at the grid's edges", () => {
    expect(move(3, "left")).toBe(3);
    expect(move(2, "right")).toBe(2);
    expect(move(6, "right")).toBe(6);
    expect(move(6, "down")).toBe(6);
  });

  it("drops onto the last card from above a short row", () => {
    expect(move(5, "down")).toBe(6);
    expect(move(3, "down")).toBe(6);
  });

  it("stays in the field with nothing found", () => {
    expect(moveSearch(FIELD, "down", 0, 3)).toBe(FIELD);
    expect(moveSearch(2, "up", 0, 3)).toBe(FIELD);
  });
});
