// Emulator settings (libretro core options) on the admin Settings page: searching them, and
// saying where each value comes from.
import type { CoreSetting, SettingSource } from "./api";

/** Settings whose description, key or value contain every word of `query`. */
export function filterSettings(settings: CoreSetting[], query: string): CoreSetting[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return settings;
  return settings.filter((s) => {
    const text = `${s.description} ${s.key} ${s.value}`.toLowerCase();
    return words.every((w) => text.includes(w));
  });
}

/** The badge beside a setting, or null for a value the core chose itself. */
export function sourceBadge(source: SettingSource): { text: string; title: string } | null {
  switch (source) {
    case "sovemu":
      return { text: "SovEmu default", title: "SovEmu sets this for the system; change it to override" };
    case "admin":
      return { text: "Changed", title: "Changed here; reset to go back to the default" };
    case "environment":
      return { text: "Environment", title: "Set by a SOVEMU_SYSTEMS__… variable on the server, which always wins" };
    default:
      return null;
  }
}

/** A choice's label in the list: the core's default is marked. */
export const choiceLabel = (choice: string, def: string) => (choice === def ? `${choice} (default)` : choice);
