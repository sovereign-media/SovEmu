import { describe, expect, it } from "vitest";
import type { JobState } from "./api";
import { formatSpan, formatTook, lastRunText, scheduleText } from "./jobs";

const NOW = 1_800_000_000_000;
const s = NOW / 1000;

function job(over: Partial<JobState> = {}): JobState {
  return {
    name: "SCAN",
    label: "Library scan",
    description: "",
    intervalMinutes: 60,
    nextRunAt: s + 600,
    running: false,
    runningSince: null,
    lastRun: null,
    ...over,
  };
}

describe("jobs", () => {
  it("says spans of time briefly", () => {
    expect(formatSpan(45)).toBe("45 s");
    expect(formatSpan(720)).toBe("12 min");
    expect(formatSpan(3 * 3600)).toBe("3 h");
    expect(formatSpan(86400)).toBe("1 day");
    expect(formatTook(0.42)).toBe("0.4 s");
    expect(formatTook(125)).toBe("2 min");
  });

  it("says when a job runs", () => {
    expect(scheduleText(job(), NOW)).toBe("Every 1 h · next in 10 min");
    expect(scheduleText(job({ running: true }), NOW)).toBe("Every 1 h");
    expect(scheduleText(job({ name: "METADATA", intervalMinutes: null, nextRunAt: null }), NOW)).toBe(
      "After each scan",
    );
    expect(scheduleText(job({ name: "RECONCILE", intervalMinutes: null, nextRunAt: null }), NOW)).toBe(
      "After each scan",
    );
    expect(scheduleText(job({ intervalMinutes: null, nextRunAt: null }), NOW)).toBe("Not scheduled");
  });

  it("says how the last run went", () => {
    expect(lastRunText(job(), NOW)).toBe("Not run yet");
    expect(lastRunText(job({ running: true, runningSince: s - 30 }), NOW)).toBe("Running for 30 s…");
    const lastRun = {
      trigger: "SCHEDULE",
      outcome: "FAILED",
      startedAt: s - 300,
      finishedAt: s - 120,
      seconds: 180,
      message: "no library",
    } as const;
    expect(lastRunText(job({ lastRun }), NOW)).toBe("Failed 2 min ago, took 3 min");
  });
});
