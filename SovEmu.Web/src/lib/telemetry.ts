// The admin Telemetry page's words. No React, so Vitest can check them.

import type { TelemetryReport } from "./api";

export type ExportResult = TelemetryReport["traces"];

/** "just now", "5 s ago", "3 min ago", "2 h ago", from Unix seconds. */
export function agoFromUnix(seconds: number, now = Date.now()): string {
  const s = Math.max(0, Math.round(now / 1000 - seconds));
  if (s < 5) return "just now";
  if (s < 60) return `${s} s ago`;
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  return `${Math.round(s / 3600)} h ago`;
}

/** A signal's state in a word and a line. */
export function exportText(r: ExportResult, now = Date.now()): { tone: "good" | "bad" | "quiet"; text: string } {
  if (r.lastAt == null) return { tone: "quiet", text: "No export yet" };
  const when = agoFromUnix(r.lastAt, now);
  const counts = `${r.exports.toLocaleString("en-US")} ${r.exports === 1 ? "export" : "exports"}, ${r.failures.toLocaleString("en-US")} failed`;
  return r.ok
    ? { tone: "good", text: `Sent ${when} · ${counts}` }
    : { tone: "bad", text: `Failed ${when} · ${counts}` };
}

/** "every 60 s", "every 1.5 s". */
export function intervalText(ms: number): string {
  const s = ms / 1000;
  return `every ${Number.isInteger(s) ? s : s.toFixed(1)} s`;
}
