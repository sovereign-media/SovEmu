// The BIOS check's wording: what each state means and what to do about a file. The server
// does the checking (names and MD5s, SovEmu.Server/src/bios.rs); this only explains it.
import type { BiosFile, BiosGroup, BiosSystem, SystemBios } from "./api";

export type Tone = "good" | "warn" | "info" | "bad" | "quiet";

export const STATES: Record<SystemBios["state"], { text: string; tone: Tone; help: string }> = {
  READY: { text: "Ready", tone: "good", help: "Everything it needs is there and verified." },
  WARNING: { text: "Check", tone: "warn", help: "Playable, but a file is not a known dump or is in the wrong place." },
  INCOMPLETE: {
    text: "Incomplete",
    tone: "info",
    help: "Playable on the core's built-in BIOS, which some games dislike.",
  },
  MISSING: { text: "Missing", tone: "bad", help: "Games will not start until a required file is added." },
};

export const IMPORTANCE: Record<BiosGroup["importance"], string> = {
  REQUIRED: "Required",
  RECOMMENDED: "Recommended",
  OPTIONAL: "Optional",
};

export const FILE_STATES: Record<BiosFile["state"], { text: string; tone: Tone }> = {
  OK: { text: "Verified", tone: "good" },
  UNVERIFIED: { text: "Unknown dump", tone: "warn" },
  MISPLACED: { text: "Misplaced", tone: "warn" },
  MISSING: { text: "Missing", tone: "bad" },
};

/** The systems that use a BIOS, in SovEmu's order. */
export function withBios(systems: BiosSystem[]): (BiosSystem & { bios: SystemBios })[] {
  return systems.filter((s): s is BiosSystem & { bios: SystemBios } => s.bios !== null);
}

/** A file's tone in its group: a missing alternative is no problem once another is there,
 *  and a missing optional file is no problem at all. */
export function fileTone(file: BiosFile, group: BiosGroup): Tone {
  if (file.state === "MISSING" && (group.satisfied || group.importance === "OPTIONAL")) return "quiet";
  if (file.state === "MISSING" && group.importance === "RECOMMENDED") return "info";
  return FILE_STATES[file.state].tone;
}

/** What to do about a file, or where it was found; null when there is nothing to say. */
export function fileHint(file: BiosFile, folder: string): string | null {
  const dest = file.name.includes("*") ? `bios/${folder}/` : `bios/${folder}/${file.name}`;
  switch (file.state) {
    case "OK":
      return file.foundAt?.startsWith("roms/") ? `Found beside the games, in ${file.foundAt}.` : null;
    case "UNVERIFIED":
      return `Its MD5 (${file.md5 ?? "?"}) is not a known dump: a bad copy, or a revision the list lacks. It may still work.`;
    case "MISPLACED": {
      const at = file.foundAt ?? "";
      const sameFolder = at.toLowerCase() === `${folder}/${file.name}`.toLowerCase();
      return sameFolder
        ? `Found as bios/${at}: rename it ${file.name} (letter case matters).`
        : `Found at bios/${at}: move it to ${dest}.`;
    }
    case "MISSING":
      return file.name.includes("*") ? `Put a dump in ${dest}` : `Put it at ${dest}`;
  }
}

/** One file of the BIOS check as a table row, with what its group and system say about it. */
export interface BiosRow {
  /** Unique within the table: system, group and file. */
  key: string;
  systemId: string;
  systemName: string;
  color: string;
  /** The system's folder in the BIOS folder (`psx`). */
  folder: string;
  systemState: SystemBios["state"];
  group: BiosGroup;
  file: BiosFile;
  tone: Tone;
  hint: string | null;
}

/** Every file of every system that uses a BIOS, in SovEmu's order. */
export function biosRows(systems: BiosSystem[]): BiosRow[] {
  return withBios(systems).flatMap((s) =>
    s.bios.groups.flatMap((group) =>
      group.files.map((file) => ({
        key: `${s.id}/${group.label}/${file.name}/${file.foundAt ?? ""}`,
        systemId: s.id,
        systemName: s.name,
        color: s.color,
        folder: s.bios.folder,
        systemState: s.bios.state,
        group,
        file,
        tone: fileTone(file, group),
        hint: fileHint(file, s.bios.folder),
      })),
    ),
  );
}

/** Keeps the rows whose system (name or id) or file name has every word of `text`, in any case. */
export function filterBiosRows(rows: BiosRow[], text: string): BiosRow[] {
  const words = text.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return rows;
  return rows.filter((r) => {
    const hay = `${r.systemName} ${r.systemId} ${r.file.name}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

export type BiosSortKey = "system" | "file" | "description" | "need" | "size" | "status";
export type SortDirection = "asc" | "desc";

/** Worst first when sorted ascending: what needs doing comes to the top. */
const TONE_ORDER: Record<Tone, number> = { bad: 0, warn: 1, info: 2, quiet: 3, good: 4 };
const NEED_ORDER: Record<BiosGroup["importance"], number> = { REQUIRED: 0, RECOMMENDED: 1, OPTIONAL: 2 };

const byText = (a: string, b: string) => a.localeCompare(b, undefined, { sensitivity: "base", numeric: true });

const COMPARE: Record<BiosSortKey, (a: BiosRow, b: BiosRow) => number> = {
  system: (a, b) => byText(a.systemName, b.systemName),
  file: (a, b) => byText(a.file.name, b.file.name),
  description: (a, b) => byText(a.file.label, b.file.label),
  need: (a, b) => NEED_ORDER[a.group.importance] - NEED_ORDER[b.group.importance],
  size: (a, b) => a.file.size - b.file.size,
  status: (a, b) => TONE_ORDER[a.tone] - TONE_ORDER[b.tone],
};

/** The rows sorted by one column. Ties keep their order (SovEmu's: by system, then file), so
 *  sorting by status still groups each system's files together. */
export function sortBiosRows(rows: BiosRow[], key: BiosSortKey, direction: SortDirection): BiosRow[] {
  const sign = direction === "asc" ? 1 : -1;
  return rows
    .map((row, i) => ({ row, i }))
    .sort((a, b) => sign * COMPARE[key](a.row, b.row) || a.i - b.i)
    .map(({ row }) => row);
}

/** The direction a column sorts in when its header is chosen: the other way when it is already
 *  the sorted column, else ascending. */
export function nextSort(
  current: { key: BiosSortKey; direction: SortDirection },
  key: BiosSortKey,
): { key: BiosSortKey; direction: SortDirection } {
  if (current.key !== key) return { key, direction: "asc" };
  return { key, direction: current.direction === "asc" ? "desc" : "asc" };
}

/** What a file's status pill says: its state, or, for a missing one, whether that matters. */
export function statusText(row: BiosRow): string {
  if (row.file.state !== "MISSING") return FILE_STATES[row.file.state].text;
  if (row.tone === "quiet") return row.group.satisfied ? "Not needed" : "Not added";
  return "Missing";
}

/** One line for the page's header: how many systems need something. */
export function summary(systems: { bios: SystemBios }[]): string {
  const missing = systems.filter((s) => s.bios.state === "MISSING").length;
  const check = systems.filter((s) => s.bios.state === "WARNING").length;
  const parts = [];
  if (missing) parts.push(`${missing} ${missing === 1 ? "system is" : "systems are"} missing a required BIOS`);
  if (check) parts.push(`${check} ${check === 1 ? "has a file" : "have files"} to check`);
  return parts.length ? `${parts.join("; ")}.` : "Every system has what it needs.";
}

/** Where a BIOS link goes: admins to the admin BIOS page (with the good dumps' MD5s), everyone
 *  else to the BIOS page; `system` scrolls to one system's card. */
export function biosHref(admin: boolean, system?: string): string {
  const page = admin ? "/admin/bios/" : "/bios/";
  return system ? `${page}?id=${encodeURIComponent(system)}` : page;
}
