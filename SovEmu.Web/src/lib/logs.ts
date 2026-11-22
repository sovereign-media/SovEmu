// The admin Logs page in words. No React, so Vitest can check them.

import type { LogLevel, LogLine } from "./api";

/** The level filter: this level and above. */
export const LEVELS: { value: LogLevel; label: string }[] = [
  { value: "DEBUG", label: "Debug" },
  { value: "INFO", label: "Info" },
  { value: "WARN", label: "Warnings" },
  { value: "ERROR", label: "Errors" },
];

/** A line's level as a short word, as log readers expect it. */
export const LEVEL_WORDS: Record<LogLevel, string> = {
  TRACE: "TRACE",
  DEBUG: "DEBUG",
  INFO: "INFO",
  WARN: "WARN",
  ERROR: "ERROR",
};

/** A line's fields as `key=value`, a value with a space quoted. */
export function fieldsText(fields: LogLine["fields"]): string {
  return fields.map((f) => `${f.key}=${/\s/.test(f.value) ? JSON.stringify(f.value) : f.value}`).join(" ");
}

/** A line's time, to the second, in the browser's time zone. */
export function timeText(at: number): string {
  return new Date(at * 1000).toLocaleTimeString("en-GB", { hour12: false });
}
