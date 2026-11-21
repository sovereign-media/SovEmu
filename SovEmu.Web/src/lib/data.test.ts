import { describe, expect, it } from "vitest";
import { q, queryClient } from "./data";
import { ApiError } from "./graphql";

describe("data", () => {
  it("retries a failed read once, but never one refused for want of signing in", () => {
    const retry = queryClient.getDefaultOptions().queries?.retry as (n: number, e: Error) => boolean;
    expect(retry(0, new Error("offline"))).toBe(true);
    expect(retry(1, new Error("offline"))).toBe(false);
    expect(retry(0, new ApiError("UNAUTHENTICATED", "Sign in"))).toBe(false);
  });

  it("keeps a game's reads under the game, so marking the game stale covers them", () => {
    const game = q.game(7).queryKey;
    for (const key of [q.saves(7).queryKey, q.patches(7).queryKey, q.cheats(7).queryKey]) {
      expect(key.slice(0, game.length)).toEqual(game);
    }
    expect(q.saves(8).queryKey.slice(0, 2)).not.toEqual(game);
  });
});
