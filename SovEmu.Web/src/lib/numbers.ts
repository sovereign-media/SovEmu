// Small sums over samples, shared by the stats overlay, the latency graph and the connection test.

/** The middle value (the upper one of an even count), or NaN for none. */
export function median(values: number[]): number {
  if (!values.length) return NaN;
  const s = [...values].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
}
