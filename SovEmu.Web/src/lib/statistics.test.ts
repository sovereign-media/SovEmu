import { describe, expect, it } from "vitest";
import { bucketLabel, buckets, playedText } from "./statistics";

const DAY = 86_400;
const monday = Date.parse("2026-11-16T00:00:00Z") / 1000;

describe("play statistics", () => {
  it("says how long was played", () => {
    expect(playedText(45 * 60)).toBe("45 min");
    expect(playedText(3.5 * 3600)).toBe("3.5 h");
    expect(playedText(2 * 3600)).toBe("2 h");
    expect(playedText(1234 * 3600)).toBe("1,234 h");
  });

  it("has a bar for every day of the window, empty or not", () => {
    const s = { from: monday + 3600, to: monday + 7 * DAY, days: [{ day: monday + DAY, sessions: 2, seconds: 600 }] };
    const bars = buckets(s, 1);
    expect(bars).toHaveLength(7);
    expect(bars[0]).toEqual({ start: monday, days: 1, seconds: 0, sessions: 0 });
    expect(bars[1]).toEqual({ start: monday + DAY, days: 1, seconds: 600, sessions: 2 });
    expect(bucketLabel(bars[1])).toBe("Tue 17 Nov");
  });

  it("groups a long window by week", () => {
    const s = {
      from: monday,
      to: monday + 10 * DAY,
      days: [
        { day: monday, sessions: 1, seconds: 60 },
        { day: monday + 6 * DAY, sessions: 1, seconds: 60 },
        { day: monday + 8 * DAY, sessions: 3, seconds: 300 },
      ],
    };
    const weeks = buckets(s, 7);
    expect(weeks.map((w) => [w.days, w.sessions, w.seconds])).toEqual([
      [7, 2, 120],
      [3, 3, 300],
    ]);
    expect(bucketLabel(weeks[0])).toBe("Week of 16 Nov");
  });
});
