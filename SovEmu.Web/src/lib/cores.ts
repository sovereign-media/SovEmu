// The admin Cores page's words and search. No React, so Vitest can check them.

import type { CoreReport } from "./api";

export type CoreState = "loaded" | "failed" | "missing";

export function coreState(c: CoreReport): CoreState {
  if (c.loaded) return "loaded";
  return c.file ? "failed" : "missing";
}

export const CORE_STATE_TEXT: Record<CoreState, string> = {
  loaded: "Loads",
  failed: "Does not load",
  missing: "Not installed",
};

/** Cores whose name, label, licence or systems match every word of `text`. */
export function filterCores(cores: CoreReport[], text: string): CoreReport[] {
  const words = text.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return cores;
  return cores.filter((c) => {
    const hay = [c.name, c.label ?? "", c.licence ?? "", ...c.systems.flatMap((s) => [s.id, s.name])]
      .join(" ")
      .toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

/** The line under the heading: "11 cores from libretro 1.22.2 · 10 load · 3 non-commercial". */
export function coresSummary(cores: CoreReport[], release: string): string {
  const loading = cores.filter((c) => c.loaded).length;
  const nonCommercial = cores.filter((c) => c.nonCommercial).length;
  return `${cores.length} cores from libretro ${release} · ${loading} load · ${nonCommercial} non-commercial`;
}
