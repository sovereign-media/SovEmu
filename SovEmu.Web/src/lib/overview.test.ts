import { describe, expect, it } from "vitest";
import type { Overview } from "./api";
import { biosGaps, diskText, diskUsed, scanText, uptimeText } from "./overview";

const now = Date.parse("2026-11-22T12:00:00Z");
const unix = now / 1000;
const job = (over: Partial<Overview["jobs"][number]>): Overview["jobs"][number] => ({
  name: "SCAN",
  running: false,
  lastRun: null,
  ...over,
});

describe("the admin overview", () => {
  it("says how long the server has been up", () => {
    expect(uptimeText(unix - 20, now)).toBe("1 min");
    expect(uptimeText(unix - 5 * 3600, now)).toBe("5 h");
    expect(uptimeText(unix - 86_400, now)).toBe("1 day");
    expect(uptimeText(unix - 3 * 86_400, now)).toBe("3 days");
  });

  it("says how the last scan went", () => {
    expect(scanText([], now).text).toBe("Not scanned yet");
    expect(scanText([job({ running: true })], now).text).toBe("Scanning now");
    const done = { outcome: "SUCCEEDED" as const, finishedAt: unix - 7200, message: null };
    expect(scanText([job({ lastRun: done })], now)).toEqual({ text: "Last scan 2 h ago", failed: false });
    expect(scanText([job({ lastRun: { ...done, outcome: "FAILED" } })], now).failed).toBe(true);
  });

  it("names systems missing a BIOS, and says the disk's space", () => {
    expect(
      biosGaps([
        { id: "psx", name: "PlayStation", biosMissing: true },
        { id: "nes", name: "NES", biosMissing: false },
      ]),
    ).toEqual(["PlayStation"]);
    const disk = { totalBytes: 4 * 1024 ** 4, freeBytes: 1024 ** 4 };
    expect(diskText(disk)).toBe("1.0 TB free of 4.0 TB");
    expect(diskUsed(disk)).toBe(75);
    expect(diskText({ totalBytes: null, freeBytes: null })).toBeNull();
  });
});
