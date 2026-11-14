import { describe, expect, it } from "vitest";
import type { SystemSummary } from "./api";
import { systemBadge } from "./systems";

const sys = (over: Partial<SystemSummary>): SystemSummary => ({
  id: "n64", name: "Nintendo 64", short: "N64", manufacturer: "Nintendo", year: 1996, color: "#000",
  game_count: 1, playable: true, needs_gpu: false, ...over,
});

describe("system badges", () => {
  it("says why a system cannot play, or that it plays slowly", () => {
    expect(systemBadge(sys({}))).toBeNull();
    expect(systemBadge(sys({ playable: false }))).toBe("no core");
    expect(systemBadge(sys({ needs_gpu: true, rendering: "gpu" }))).toBeNull();
    expect(systemBadge(sys({ needs_gpu: true, rendering: "software" }))).toBe("software rendering");
    expect(systemBadge(sys({ needs_gpu: true, rendering: "off", playable: false }))).toBe("no 3D rendering");
  });
});
