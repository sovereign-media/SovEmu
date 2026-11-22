// The admin Backups page in words. No React, so Vitest can check them.

import type { Backups } from "./api";

/** When backups are made on their own. */
export function scheduleText(b: Backups): string {
  if (b.intervalHours <= 0) return "Only when you back up now: SOVEMU_BACKUP_INTERVAL_HOURS is 0.";
  const every = b.intervalHours === 1 ? "Every hour" : `Every ${b.intervalHours} hours`;
  return `${every}, keeping ${b.keep === 0 ? "every one" : `the newest ${b.keep}`}.`;
}

/** The setting that restores `name` from the backup folder at the next start. */
export function restoreLine(b: Backups, name: string): string {
  return `SOVEMU_RESTORE_FROM=${b.dir.replace(/\/+$/, "")}/${name}`;
}
