// The admin Telemetry page's words. No React, so Vitest can check them.

import type { TelemetryReport } from "./api";
import { ago } from "./time";

export type ExportResult = TelemetryReport["traces"];

/** A signal's state in a word and a line. */
export function exportText(r: ExportResult, now = Date.now()): { tone: "good" | "bad" | "quiet"; text: string } {
  if (r.lastAt == null) return { tone: "quiet", text: "No export yet" };
  const when = ago(r.lastAt, now, { seconds: true });
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
