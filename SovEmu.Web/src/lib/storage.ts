// The admin Storage page's words and the segments of its bar. No React, so Vitest can check them.

import type { StorageReport } from "./api";

export type StorageArea = StorageReport["areas"][number];

export const AREA_LABELS: Record<string, string> = {
  database: "Database",
  saves: "Saves",
  cache: "Game cache",
  discs: "Disc playlists",
  mirror: "Library mirror",
  artwork: "Box art",
  launchbox: "LaunchBox database",
  metadata: "Other game information",
  decorations: "Your decorations",
  builtin_decorations: "Built-in decorations",
  roms: "Library",
  bios: "BIOS files",
  backups: "Backups",
  other: "Other",
};

export interface Segment {
  id: string;
  label: string;
  bytes: number;
  /** Of the filesystem, 0-100. */
  percent: number;
}

/**
 * The bar: SovEmu's areas, then whatever else is on the filesystem, then the free space, each as
 * a share of the filesystem. Empty without the filesystem's size.
 */
export function segments(r: StorageReport): Segment[] {
  if (!r.totalBytes) return [];
  const total = r.totalBytes;
  const ours = r.areas.filter((a) => a.bytes > 0);
  const used = ours.reduce((sum, a) => sum + a.bytes, 0);
  const free = r.freeBytes ?? 0;
  const elsewhere = Math.max(0, total - free - used);
  const pct = (b: number) => (b / total) * 100;
  return [
    ...ours.map((a) => ({ id: a.id, label: AREA_LABELS[a.id] ?? a.id, bytes: a.bytes, percent: pct(a.bytes) })),
    { id: "elsewhere", label: "Not SovEmu's", bytes: elsewhere, percent: pct(elsewhere) },
    { id: "free", label: "Free", bytes: free, percent: pct(free) },
  ];
}

/** A path within the data folder, shown relative to it (`saves`, `cache/games`); `.` for the folder. */
export function relativePath(path: string, dataDir: string): string {
  const root = dataDir.replace(/\/+$/, "");
  if (path === root) return ".";
  return path.startsWith(`${root}/`) ? path.slice(root.length + 1) : path;
}
