import { describe, expect, it } from "vitest";
import { ago, agoFromSqlite } from "./time";

describe("time ago", () => {
  const now = Date.parse("2026-11-14T12:00:00Z");
  const secondsAgo = (s: number) => now / 1000 - s;

  it("counts in minutes, hours and days", () => {
    expect(ago(secondsAgo(20), now)).toBe("just now");
    expect(ago(secondsAgo(180), now)).toBe("3 min ago");
    expect(ago(secondsAgo(7200), now)).toBe("2 h ago");
    expect(ago(secondsAgo(86_400), now)).toBe("yesterday");
    expect(ago(secondsAgo(4 * 86_400), now)).toBe("4 days ago");
    // A client clock a little ahead.
    expect(ago(secondsAgo(-30), now)).toBe("just now");
  });

  it("counts seconds when asked", () => {
    expect(ago(secondsAgo(2), now, { seconds: true })).toBe("just now");
    expect(ago(secondsAgo(30), now, { seconds: true })).toBe("30 s ago");
  });

  it("reads SQLite's timestamps", () => {
    expect(agoFromSqlite(null, now)).toBe("never");
    expect(agoFromSqlite("2026-11-14 11:59:40", now)).toBe("just now");
    expect(agoFromSqlite("2026-11-14 11:30:00", now)).toBe("30 min ago");
    expect(agoFromSqlite("2026-11-13 12:00:00", now)).toBe("yesterday");
    expect(agoFromSqlite("2026-11-10 12:00:00", now)).toBe("4 days ago");
  });
});
