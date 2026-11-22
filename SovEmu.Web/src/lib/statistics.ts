// The admin Statistics page's sums and words. No React, so Vitest can check them.

import type { PlayStatistics, StatsRange } from "./api";

export const RANGES: { value: StatsRange; label: string }[] = [
  { value: "WEEK", label: "Week" },
  { value: "MONTH", label: "Month" },
  { value: "YEAR", label: "Year" },
];

/** Time played in words: "45 min", "3.5 h", "120 h". */
export function playedText(seconds: number): string {
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = seconds / 3600;
  return `${hours < 10 ? hours.toFixed(1).replace(/\.0$/, "") : Math.round(hours).toLocaleString("en-US")} h`;
}

/** One bar of the chart: a day, or a week for a year's window. */
export interface Bucket {
  /** Its start, Unix seconds (UTC). */
  start: number;
  days: number;
  seconds: number;
  sessions: number;
}

const DAY = 86_400;

/**
 * The window's days as bars, every one present (a day without plays is an empty bar, not a
 * gap), grouped by `size` days: a day for a week or a month, a week for a year.
 */
export function buckets(s: Pick<PlayStatistics, "from" | "to" | "days">, size: number): Bucket[] {
  const first = Math.floor(s.from / DAY) * DAY;
  const last = Math.floor((s.to - 1) / DAY) * DAY;
  const out: Bucket[] = [];
  for (let start = first; start <= last; start += size * DAY) {
    out.push({ start, days: Math.min(size, (last - start) / DAY + 1), seconds: 0, sessions: 0 });
  }
  for (const d of s.days) {
    const b = out[Math.floor((d.day - first) / (size * DAY))];
    if (b) {
      b.seconds += d.seconds;
      b.sessions += d.sessions;
    }
  }
  return out;
}

/** How many days a bar covers for each window. */
export const BUCKET_DAYS: Record<StatsRange, number> = { WEEK: 1, MONTH: 1, YEAR: 7 };

/** A bar's label: "Mon 17 Nov", or for a week "Week of 17 Nov". */
export function bucketLabel(b: Bucket): string {
  const date = new Date(b.start * 1000);
  const day = date.toLocaleDateString("en-GB", { day: "numeric", month: "short", timeZone: "UTC" });
  if (b.days > 1) return `Week of ${day}`;
  return `${date.toLocaleDateString("en-GB", { weekday: "short", timeZone: "UTC" })} ${day}`;
}
