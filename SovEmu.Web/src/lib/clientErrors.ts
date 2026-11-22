// The admin Client errors page in words: what each kind means, which are faults, and how a
// group's clients and times read. No React, so Vitest can check them.

import type { ClientEventKind } from "./api";

export const KIND_LABELS: Record<ClientEventKind, string> = {
  CONNECTION_FAILED: "Could not connect",
  CONNECTION_LOST: "Connection lost",
  STREAM_STALLED: "Picture stalled",
  CONTROLLER_UNKNOWN: "Unknown controller",
  SCREEN_ERROR: "Screen error",
  OTHER: "Other",
};

/** What went wrong, as opposed to what is worth knowing (an unknown controller, other notes). */
export function isFault(kind: ClientEventKind): boolean {
  return kind !== "CONTROLLER_UNKNOWN" && kind !== "OTHER";
}

/** The windows the page offers, in days. */
export const WINDOWS = [
  { value: "1", label: "Day" },
  { value: "7", label: "Week" },
  { value: "30", label: "Month" },
] as const;

/** `android 1.4.0 (Mi TV, Android 11)` as its app (`android 1.4.0`) and its device (`Mi TV, Android 11`). */
export function splitClient(client: string): { app: string; device: string | null } {
  const m = /^(.*?)\s*\((.*)\)\s*$/.exec(client);
  return m ? { app: m[1] || client, device: m[2] || null } : { app: client, device: null };
}
