// The Library page's words for a game-information lookup: what it is doing now, how long it has
// left, what each source found and its quota. No React, so Vitest can check them.

import type { MetadataStatus } from "./api";

export type Provider = MetadataStatus["providers"][number];
export type Recent = MetadataStatus["recent"][number];

const n = (x: number) => x.toLocaleString("en-US");

/** A source's name, from its id (`screenscraper` → "ScreenScraper"). */
export function sourceName(status: Pick<MetadataStatus, "providers">, id: string): string {
  return status.providers.find((p) => p.id === id)?.name ?? id;
}

/** "about 12 minutes left", from seconds. */
export function etaText(seconds: number | null): string {
  if (seconds === null) return "";
  if (seconds < 60) return "less than a minute left";
  const minutes = Math.round(seconds / 60);
  if (minutes < 90) return `about ${minutes} minute${minutes === 1 ? "" : "s"} left`;
  const hours = Math.round(minutes / 60);
  return `about ${hours} hours left`;
}

/** The overall line under the progress bar. */
export function progressText(s: MetadataStatus): string {
  if (s.running) {
    const eta = etaText(s.etaSeconds);
    return `Looking up ${n(s.done)} of ${n(s.total)} games${eta ? ` · ${eta}` : ""}…`;
  }
  if (s.done) return `Last run: ${n(s.found)} found, ${n(s.notFound)} not found, ${n(s.failed)} to retry.`;
  return "";
}

/** What the lookup is doing for the game it is on. */
export function currentText(s: MetadataStatus): string | null {
  const c = s.current;
  if (!c) return null;
  const source = c.provider ? sourceName(s, c.provider) : null;
  const doing =
    c.step === "HASHING"
      ? "hashing the file"
      : c.step === "WAITING"
        ? `waiting for ${source ?? "the service"}`
        : source
          ? `asking ${source}`
          : "starting";
  return `${c.title} (${c.system}): ${doing}`;
}

/** A source's quota today, if it reports one. */
export function quotaText(p: Provider): string {
  const q = p.quota;
  if (!q) return "";
  const of = (used: number, max: number | null) => (max ? `${n(used)} of ${n(max)}` : n(used));
  const misses = q.missesToday || q.maxMissesPerDay ? ` · ${of(q.missesToday, q.maxMissesPerDay)} not found` : "";
  return `${of(q.requestsToday, q.maxRequestsPerDay)} requests today${misses}`;
}

/** How far into its daily quota a source is, 0-100, if it says. */
export function quotaPercent(p: Provider): number | null {
  const q = p.quota;
  if (!q?.maxRequestsPerDay) return null;
  return Math.min(100, (100 * q.requestsToday) / q.maxRequestsPerDay);
}

/** One recent game's result. */
export function recentText(s: Pick<MetadataStatus, "providers">, r: Recent): string {
  switch (r.outcome) {
    case "FOUND":
      return `found by ${r.sources.map((id) => sourceName(s, id)).join(" and ")}`;
    case "NOT_FOUND":
      return "not found";
    default:
      return r.detail ? `to retry: ${r.detail}` : "to retry";
  }
}
