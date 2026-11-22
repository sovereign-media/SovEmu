// Sign-ins in words, for the admin's Devices page and a user's page. No React, so Vitest can
// check them.

import type { SignIn } from "./api";
import { ago } from "./time";

export const CLIENT_LABELS: Record<SignIn["client"], string> = {
  WEB: "Web browser",
  APP: "App",
  OTHER: "Other client",
};

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
