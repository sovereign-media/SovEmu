// The admin Configuration page's search, grouping and "copy as environment lines". No React, so
// Vitest can check them.

import type { ConfigSetting } from "./api";

export type ConfigGroup = ConfigSetting["group"];

export const GROUP_LABELS: Record<ConfigGroup, string> = {
  ACCOUNTS: "Accounts",
  LIBRARY: "Library",
  GAME_INFORMATION: "Game information",
  STREAMING: "Streaming",
  OPERATIONS: "Operations",
};

export const SOURCE_LABELS: Record<ConfigSetting["source"], string> = {
  DEFAULT: "default",
  ENVIRONMENT: "environment",
  FILE: "file",
  COMMAND_LINE: "command line",
};

/** Settings whose name, value or description hold every word of `text`. */
export function filterSettings(settings: ConfigSetting[], text: string): ConfigSetting[] {
  const words = text.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return settings;
  return settings.filter((s) => {
    const hay = `${s.name} ${s.value ?? ""} ${s.effective ?? ""} ${s.description}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

/** Settings by group, in the server's order (the README's), leaving out empty groups. */
export function groupSettings(settings: ConfigSetting[]): { group: ConfigGroup; settings: ConfigSetting[] }[] {
  const groups: { group: ConfigGroup; settings: ConfigSetting[] }[] = [];
  for (const s of settings) {
    const last = groups.at(-1);
    if (last?.group === s.group) last.settings.push(s);
    else groups.push({ group: s.group, settings: [s] });
  }
  return groups;
}

/**
 * The settings this server was given, as environment lines to keep: everything not left at its
 * default, but secrets, which are only ever shown as set or unset.
 */
export function environmentLines(settings: ConfigSetting[]): string[] {
  return settings
    .filter((s) => s.source !== "DEFAULT" && !s.secret && s.value !== null)
    .map((s) => `${s.name}=${s.value}`);
}

/** Values longer than this are folded, with a button to show them whole. */
export const LONG_VALUE = 80;

/** A doc comment's text split at its backticks: `code` spans and plain text, in order. */
export function codeSpans(text: string): { code: boolean; text: string }[] {
  return text
    .split("`")
    .map((part, i) => ({ code: i % 2 === 1, text: part }))
    .filter((p) => p.text !== "");
}
