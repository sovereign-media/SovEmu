// The About page's open-source packages, by licence. No React, so Vitest can check them.

import type { Notice } from "./api";

/** One licence: its texts (they differ by copyright line), each with its packages. */
export interface LicenceGroup {
  id: string;
  packages: number;
  texts: Notice[];
}

/** The notices by licence, the most used first. */
export function byLicence(notices: Notice[]): LicenceGroup[] {
  const groups = new Map<string, LicenceGroup>();
  for (const n of notices) {
    const g = groups.get(n.id) ?? { id: n.id, packages: 0, texts: [] };
    g.packages += n.packages.length;
    g.texts.push(n);
    groups.set(n.id, g);
  }
  return [...groups.values()].sort((a, b) => b.packages - a.packages || a.id.localeCompare(b.id));
}

/** "axum 0.8.4, tokio 1.48.0": a text's packages. */
export function packagesText(n: Notice): string {
  return n.packages.map((p) => `${p.name} ${p.version}`).join(", ");
}
