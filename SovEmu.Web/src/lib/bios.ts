// The BIOS check's wording: what each state means and what to do about a file. The server
// does the checking (names and MD5s, SovEmu.Server/src/bios.rs); this only explains it.
import type { BiosFile, BiosGroup, BiosSystem, SystemBios } from "./api";

export type Tone = "good" | "warn" | "info" | "bad" | "quiet";

export const STATES: Record<SystemBios["state"], { text: string; tone: Tone; help: string }> = {
  READY: { text: "Ready", tone: "good", help: "Everything it needs is there and verified." },
  WARNING: { text: "Check", tone: "warn", help: "Playable, but a file is not a known dump or is in the wrong place." },
  INCOMPLETE: { text: "Incomplete", tone: "info", help: "Playable on the core's built-in BIOS, which some games dislike." },
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

/** One line for the page's header: how many systems need something. */
export function summary(systems: { bios: SystemBios }[]): string {
  const missing = systems.filter((s) => s.bios.state === "MISSING").length;
  const check = systems.filter((s) => s.bios.state === "WARNING").length;
  const parts = [];
  if (missing) parts.push(`${missing} ${missing === 1 ? "system is" : "systems are"} missing a required BIOS`);
  if (check) parts.push(`${check} ${check === 1 ? "has a file" : "have files"} to check`);
  return parts.length ? `${parts.join("; ")}.` : "Every system has what it needs.";
}
