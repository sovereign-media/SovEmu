// Small sums and figures shared across the app: the median of samples (the stats overlay, the
// latency graph, the connection test) and sizes in bytes (every page that shows one).

/** The middle value (the upper one of an even count), or NaN for none. */
export function median(values: number[]): number {
  if (!values.length) return NaN;
  const s = [...values].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
}

/**
 * A size in bytes, 1024-based, one decimal under 10 and none from 10: "512 B", "1.5 KB", "64 KB",
 * "4.0 MB", "3.3 GB", "128 GB".
 */
export function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let n = bytes;
  let unit = 0;
  while (n >= 1024 && unit < units.length - 1) {
    n /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${n} B` : `${n.toFixed(n < 10 ? 1 : 0)} ${units[unit]}`;
}
