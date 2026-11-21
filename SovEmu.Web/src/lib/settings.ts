// Emulator settings (libretro core options) on the admin Settings page: grouping them, searching
// them, and saying where each value comes from.
import type { CoreCategory, CoreSetting, SettingSource } from "./api";

/** Settings whose description, explanation, key or value contain every word of `query`. */
export function filterSettings(settings: CoreSetting[], query: string): CoreSetting[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return settings;
  return settings.filter((s) => {
    const text = `${s.description} ${s.info ?? ""} ${s.key} ${s.value}`.toLowerCase();
    return words.every((w) => text.includes(w));
  });
}

/** A section of the page: a core's own category, or one SovEmu guessed. */
export interface SettingGroup {
  id: string;
  title: string;
  info: string;
  settings: CoreSetting[];
}

/** SovEmu's groups for settings a core leaves uncategorised, tried in this order: the first
 *  whose pattern matches the setting's key or description wins. */
const GUESSES: { title: string; info: string; pattern: RegExp }[] = [
  {
    title: "Audio",
    info: "Sound output, channels and volume.",
    pattern: /audio|sound|volume|\bapu\b|\bspu\b|reverb|sample.?rate|music|\bsfx\b|channel|\bpsg\b|\bdsp\b|\bfm\b/,
  },
  {
    title: "Input",
    info: "Controllers, light guns, mice and how buttons behave.",
    pattern: /input|controller|joypad|gamepad|analog|mouse|light.?gun|\bgun\b|crosshair|turbo|rumble|button|keyboard|dead.?zone|multitap|\bstick\b|touch/,
  },
  {
    title: "Video",
    info: "Resolution, aspect ratio, filtering and what is drawn.",
    pattern: /video|render|resolution|aspect|crop|overscan|palette|colou?r|shader|filter|scal|texture|widescreen|graphic|gfx|display|screen|blend|scanline|ntsc|frame.?skip|sprite|layer|bilinear|alias|msaa|vsync|gamma|brightness|interlac|pixel|dither|\bfog\b|mipmap|anisotrop|framebuffer|\bhud\b/,
  },
  {
    title: "Performance and hacks",
    info: "Speed, accuracy and workarounds for particular games.",
    pattern: /hack|speed|fast|accura|dynarec|recompil|\bjit\b|thread|cache|timing|latency|run.?ahead|cycle|overclock|underclock/,
  },
  {
    title: "System",
    info: "The emulated console: region, BIOS, memory and hardware model.",
    pattern: /region|bios|system|model|hardware|\bcpu\b|clock|memory|\bram\b|boot|language|console|cart|dis[ck]|\bcd\b|save|\brtc\b|date|time/,
  },
];
const OTHER = { title: "Other", info: "Settings that fit no other group." };
/** The groups every system shows first, in this order. */
const MAIN = ["system", "video", "audio", "input"];

/** The group SovEmu puts an uncategorised setting in. */
export function guessGroup(s: Pick<CoreSetting, "key" | "description">): string {
  const text = `${s.key} ${s.description}`.toLowerCase().replace(/[_-]/g, " ");
  return GUESSES.find((g) => g.pattern.test(text))?.title ?? OTHER.title;
}

/** The page's sections: the core's categories, and SovEmu's guesses for the settings it leaves
 *  out (merged into a core category of the same name), each keeping the core's order of
 *  settings. Empty groups are left out. */
export function groupSettings(settings: CoreSetting[], categories: CoreCategory[]): SettingGroup[] {
  const groups = new Map<string, SettingGroup>();
  const byTitle = new Map<string, SettingGroup>();
  const add = (id: string, title: string, info: string) => {
    const g = { id, title, info, settings: [] };
    groups.set(id, g);
    byTitle.set(title.toLowerCase(), g);
    return g;
  };
  for (const c of categories) add(`core-${c.key}`, c.description || c.key, c.info ?? "");
  for (const s of settings) {
    const own = s.category ? groups.get(`core-${s.category}`) : undefined;
    if (own) {
      own.settings.push(s);
      continue;
    }
    const title = guessGroup(s);
    const guess = [...GUESSES, OTHER].find((g) => g.title === title)!;
    const target = byTitle.get(title.toLowerCase()) ?? add(`guess-${title.toLowerCase().replace(/\W+/g, "-")}`, title, guess.info);
    target.settings.push(s);
  }
  // The same order on every system: System, Video, Audio, Input (the core's or SovEmu's), then
  // the core's other categories in its order, then SovEmu's remaining guesses, Other last.
  const rank = (g: SettingGroup) => {
    const main = MAIN.indexOf(g.title.toLowerCase());
    if (main >= 0) return main;
    if (g.id.startsWith("core-")) return MAIN.length;
    return MAIN.length + 1 + [...GUESSES, OTHER].findIndex((x) => x.title === g.title);
  };
  return [...groups.values()]
    .filter((g) => g.settings.length > 0)
    .map((g, i) => ({ g, i }))
    .sort((a, b) => rank(a.g) - rank(b.g) || a.i - b.i)
    .map(({ g }) => g);
}

/** The badge beside a setting, or null for a value the core chose itself. */
export function sourceBadge(source: SettingSource): { text: string; title: string } | null {
  switch (source) {
    case "SOVEMU":
      return { text: "SovEmu default", title: "SovEmu sets this for the system; change it to override" };
    case "ADMIN":
      return { text: "Changed", title: "Changed here; reset to go back to the default" };
    case "ENVIRONMENT":
      return { text: "Environment", title: "Set by a SOVEMU_SYSTEMS__… variable on the server, which always wins" };
    default:
      return null;
  }
}

/** A choice's label in the list: the core's own label, with its default marked. */
export function choiceLabel(s: Pick<CoreSetting, "choices" | "labels" | "default">, choice: string): string {
  const i = s.choices.indexOf(choice);
  const label = (i >= 0 ? s.labels?.[i] : undefined) || choice;
  return choice === s.default ? `${label} (default)` : label;
}

/**
 * The run-ahead choices, 0 (off) to `max` frames, each with the input lag it hides at 60 fps.
 * Run-ahead shows the picture that many frames ahead of the game, so a press answers sooner.
 */
export function runAheadChoices(max: number): { value: string; label: string }[] {
  return Array.from({ length: max + 1 }, (_, frames) => ({
    value: String(frames),
    label: frames === 0 ? "Off" : `${frames} frame${frames > 1 ? "s" : ""} (~${Math.round((frames * 1000) / 60)} ms)`,
  }));
}
