// The admin Activity page in words, and its CSV export. No React, so Vitest can check them.

import type { ActivityCategory, ActivityEntry } from "./api";

export const KIND_LABELS: Record<ActivityEntry["kind"], string> = {
  SIGNED_IN: "Signed in",
  SIGN_IN_FAILED: "Sign-in refused",
  SIGN_IN_LOCKED_OUT: "Sign-in locked out",
  GAME_STARTED: "Started a game",
  GAME_ENDED: "Game ended",
  USER_ADDED: "Added an account",
  ROLE_CHANGED: "Changed a role",
  PASSWORD_SET: "Set a password",
  USER_REMOVED: "Removed an account",
  SIGNED_OUT_EVERYWHERE: "Signed an account out everywhere",
  SESSION_STOPPED: "Stopped a game",
  SETTINGS_CHANGED: "Changed settings",
  JOB_RUN: "Ran a job",
  CACHE_CLEARED: "Cleared the cache",
};

/** The page's filter: every kind, or one category. */
export const CATEGORIES: { value: ActivityCategory | "ALL"; label: string }[] = [
  { value: "ALL", label: "All" },
  { value: "SIGN_IN", label: "Sign-ins" },
  { value: "PLAY", label: "Play" },
  { value: "ADMIN", label: "Admin changes" },
];

/** What went wrong, as opposed to what simply happened. */
export function isWarning(kind: ActivityEntry["kind"]): boolean {
  return kind === "SIGN_IN_FAILED" || kind === "SIGN_IN_LOCKED_OUT";
}

/** One field of a CSV line: quoted when it holds a comma, a quote or a line break. */
function field(value: string | number | null | undefined): string {
  const text = value == null ? "" : String(value);
  return /[",\r\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
}

/** The entries as CSV (RFC 4180), with a header line and times in ISO 8601 (UTC). */
export function toCsv(entries: ActivityEntry[]): string {
  const header = ["time", "user", "what", "subject", "detail", "address"];
  const lines = entries.map((e) =>
    [new Date(e.at * 1000).toISOString(), e.username, KIND_LABELS[e.kind], e.subject, e.detail, e.address]
      .map(field)
      .join(","),
  );
  return `${[header.join(","), ...lines].join("\r\n")}\r\n`;
}
