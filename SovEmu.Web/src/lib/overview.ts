// The admin Overview's cards in words. No React, so Vitest can check them.

import type { Overview } from "./api";
import { formatBytes } from "./storage";
import { ago } from "./time";

/** How long the server has been up: "3 min", "5 h", "2 days". */
export function uptimeText(startedAt: number, now = Date.now()): string {
  const s = Math.max(0, Math.round(now / 1000 - startedAt));
  if (s < 3600) return `${Math.max(1, Math.round(s / 60))} min`;
  if (s < 86_400) return `${Math.round(s / 3600)} h`;
  const days = Math.round(s / 86_400);
  return `${days} ${days === 1 ? "day" : "days"}`;
}

/** The last library scan: "Scanning now", "Last scan 2 h ago", or how it failed. */
export function scanText(jobs: Overview["jobs"], now = Date.now()): { text: string; failed: boolean } {
  const scan = jobs.find((j) => j.name === "SCAN");
  if (scan?.running) return { text: "Scanning now", failed: false };
  const run = scan?.lastRun;
  if (!run?.finishedAt) return { text: "Not scanned yet", failed: false };
  const when = ago(run.finishedAt, now);
  return run.outcome === "SUCCEEDED"
    ? { text: `Last scan ${when}`, failed: false }
    : { text: `Last scan failed ${when}`, failed: true };
}

/** The systems missing a BIOS they need, by name. */
export function biosGaps(systems: Overview["systems"]): string[] {
  return systems.filter((s) => s.biosMissing).map((s) => s.name);
}

/** "512 GB free of 2.0 TB", or null when the filesystem is not known. */
export function diskText(storage: Overview["storage"]): string | null {
  if (storage.totalBytes == null || storage.freeBytes == null) return null;
  return `${formatBytes(storage.freeBytes)} free of ${formatBytes(storage.totalBytes)}`;
}

/** The share of the disk in use, 0 to 100, or null when not known. */
export function diskUsed(storage: Overview["storage"]): number | null {
  if (!storage.totalBytes || storage.freeBytes == null) return null;
  return Math.round(((storage.totalBytes - storage.freeBytes) / storage.totalBytes) * 100);
}
