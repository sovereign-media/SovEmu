// How long ago something happened, in words, one way across the app: saves, recently played,
// job runs, exports, client errors.

/**
 * "just now", "3 min ago", "2 h ago", "yesterday", "4 days ago", from Unix seconds. With
 * `seconds`, under a minute counts in seconds ("30 s ago") for what changes that often.
 */
export function ago(unix: number, now = Date.now(), { seconds = false } = {}): string {
  const s = Math.max(0, Math.round(now / 1000 - unix));
  if (s < (seconds ? 5 : 60)) return "just now";
  if (s < 60) return `${s} s ago`;
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86_400) return `${Math.round(s / 3600)} h ago`;
  const days = Math.round(s / 86_400);
  return days === 1 ? "yesterday" : `${days} days ago`;
}

/** [`ago`] from an SQLite UTC timestamp ("2026-11-13 23:00:00"); "never" for none. */
export function agoFromSqlite(sqliteUtc: string | null, now = Date.now()): string {
  if (!sqliteUtc) return "never";
  return ago(Date.parse(`${sqliteUtc.replace(" ", "T")}Z`) / 1000, now);
}
