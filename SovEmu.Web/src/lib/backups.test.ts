import { describe, expect, it } from "vitest";
import type { Backups } from "./api";
import { restoreLine, scheduleText } from "./backups";

const backups = (over: Partial<Backups>): Backups => ({
  dir: "/data/backups",
  intervalHours: 0,
  keep: 7,
  restoreFrom: null,
  files: [],
  ...over,
});

describe("backups", () => {
  it("says when backups are made", () => {
    expect(scheduleText(backups({}))).toBe("Only when you back up now: SovEmu__Backups__IntervalHours is 0.");
    expect(scheduleText(backups({ intervalHours: 24 }))).toBe("Every 24 hours, keeping the newest 7.");
    expect(scheduleText(backups({ intervalHours: 1, keep: 0 }))).toBe("Every hour, keeping every one.");
  });

  it("names the setting that restores a backup", () => {
    expect(restoreLine(backups({ dir: "/backups/" }), "sovemu-backup-20261122-163005.zip")).toBe(
      "SovEmu__Backups__RestoreFrom=/backups/sovemu-backup-20261122-163005.zip",
    );
  });
});
