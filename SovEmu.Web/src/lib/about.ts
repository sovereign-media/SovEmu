// The admin About page's words. No React, so Vitest can check them.

import type { About } from "./api";

/** "0.1.0 · a1b2c3d · built 22 Nov 2026", or "0.1.0 · local build". */
export function buildText(b: Pick<About["buildInfo"], "version" | "revision" | "built">): string {
  if (!b.revision) return `${b.version} · local build`;
  const parts = [b.version, b.revision.slice(0, 7)];
  if (b.built) {
    const date = new Date(b.built);
    if (!Number.isNaN(date.getTime()))
      parts.push(`built ${date.toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric" })}`);
  }
  return parts.join(" · ");
}

/** A commit, shortened to its first 12 characters. */
export function shortCommit(commit: string): string {
  return commit.slice(0, 12);
}
