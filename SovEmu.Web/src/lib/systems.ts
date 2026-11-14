import type { SystemSummary } from "./api";

/** The small status badge on a system's card, if it needs one. */
export function systemBadge(s: SystemSummary): string | null {
  if (s.needs_gpu && s.rendering === "off") return "no 3D rendering";
  if (!s.playable) return "no core";
  if (s.needs_gpu && s.rendering === "software") return "software rendering";
  return null;
}

/** A maker and its systems, for the Systems menu. */
export interface MakerGroup {
  maker: string;
  systems: SystemSummary[];
}

/** Systems grouped by maker, in SovEmu's order of systems (a maker first appears where its
 *  first system does). A maker with a single system is still a group; the menu shows it as a
 *  plain entry rather than a submenu of one. */
export function groupByMaker(systems: SystemSummary[]): MakerGroup[] {
  const groups: MakerGroup[] = [];
  for (const s of systems) {
    const group = groups.find((g) => g.maker === s.manufacturer);
    if (group) group.systems.push(s);
    else groups.push({ maker: s.manufacturer, systems: [s] });
  }
  return groups;
}

/** Up to two initials for an account's avatar: "Bruce Lee" → "BL", "mum" → "M". */
export function initials(name: string): string {
  const parts = name.trim().split(/[\s._-]+/).filter(Boolean);
  const letters = parts.length > 1 ? parts[0][0] + parts[1][0] : (parts[0] ?? "?")[0];
  return letters.toUpperCase();
}
