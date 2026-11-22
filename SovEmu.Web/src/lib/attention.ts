// The admin page "Needs attention" in words, and its counts summed. No React, so Vitest can check
// them.

import type { AttentionCount, AttentionItem, AttentionKind } from "./api";

/** Each kind's tab label and what it means, in the order the tabs show. */
export const KINDS: { value: AttentionKind; label: string; help: string }[] = [
  {
    value: "NO_INFORMATION",
    label: "No information",
    help: "Games with no description, year or publisher: never looked up, or not found.",
  },
  { value: "NO_BOX_ART", label: "No box art", help: "Games with no picture beside them, and none downloaded." },
  {
    value: "DUPLICATE",
    label: "Duplicates",
    help: "The same title with the same tags in two or more files of a system.",
  },
  { value: "MISSING_PARTS", label: "Missing discs", help: "Games of several discs or sides with one missing." },
  {
    value: "UNRECOGNISED",
    label: "Not games",
    help: "Files in the library the last scan could not place: no system plays them where they are.",
  },
];

/** Whether looking a game up again could help: it lacks what a lookup fetches. */
export function canLookUp(kind: AttentionKind): boolean {
  return kind === "NO_INFORMATION" || kind === "NO_BOX_ART";
}

/** The kinds' totals, and one kind's count per system (empty: a folder no system uses). */
export function tally(counts: AttentionCount[], kind: AttentionKind) {
  const totals = Object.fromEntries(KINDS.map((k) => [k.value, 0])) as Record<AttentionKind, number>;
  for (const c of counts) totals[c.kind] += c.count;
  const systems = counts.filter((c) => c.kind === kind).map((c) => ({ system: c.system, count: c.count }));
  return { totals, systems };
}

/** An item's name: the game's title, else (a file) its name. */
export function itemName(item: AttentionItem): string {
  return item.title || item.path.slice(item.path.lastIndexOf("/") + 1);
}

/** An item's detail, in words. */
export function detailText(kind: AttentionKind, item: AttentionItem): string {
  switch (kind) {
    case "NO_INFORMATION":
      return item.detail === "not found" ? "Looked up, not found" : "Never looked up";
    case "DUPLICATE":
      return item.detail ? `Tags: ${item.detail}` : "No tags";
    case "MISSING_PARTS":
      return `Missing ${item.detail}`;
    default:
      return item.detail;
  }
}

/** The kind a URL's `kind` names, else the first. */
export function kindFrom(value: string | null): AttentionKind {
  return KINDS.find((k) => k.value === value)?.value ?? KINDS[0].value;
}
