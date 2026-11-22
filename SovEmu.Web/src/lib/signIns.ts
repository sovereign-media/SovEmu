// Sign-ins in words, for the admin's Devices page and a user's page. No React, so Vitest can
// check them.

import type { SignIn } from "./api";
import { ago } from "./time";

export const CLIENT_LABELS: Record<SignIn["client"], string> = {
  WEB: "Web browser",
  APP: "App",
  OTHER: "Other client",
};

/**
 * What a sign-in is called: the device's name, as its app gave it ("SovEmu for Android 1.4.0 ·
 * Mi TV"), else its kind of client. `lower` for the middle of a sentence (a device's own name
 * is kept as written).
 */
export function signInName(s: Pick<SignIn, "client" | "device">, { lower = false } = {}): string {
  if (s.device) return s.device;
  const label = CLIENT_LABELS[s.client];
  return lower ? label.toLowerCase() : label;
}

/** "Lapses in 3 days" from Unix seconds, or that it does not; "Lapsed" once past. */
export function lapsesText(expiresAt: number | null, now = Date.now()): string {
  if (expiresAt == null) return "Stays signed in until signed out";
  const days = Math.round((expiresAt - now / 1000) / 86_400);
  if (days < 0) return "Lapsed";
  if (days === 0) return "Lapses today unless used";
  return `Lapses in ${days === 1 ? "a day" : `${days} days`} unless used`;
}

/** "Signed in 4 days ago · last used just now". */
export function usedText(s: Pick<SignIn, "signedInAt" | "lastUsedAt">, now = Date.now()): string {
  return `Signed in ${ago(s.signedInAt, now)} · last used ${ago(s.lastUsedAt, now)}`;
}
