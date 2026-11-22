// Every screen's path, in one list: the router gives each a screen (`router.tsx`, typed so that
// neither can have one the other lacks), the admin menu links only to these, and the end-to-end
// tests open each one by its URL alone, as a bookmark or a reload would.

export const SCREEN_PATHS = [
  "/",
  "/login/",
  "/auth/callback/",
  "/system/",
  "/play/",
  "/search/",
  "/bios/",
  "/admin/overview/",
  "/admin/",
  "/admin/users/",
  "/admin/settings/",
  "/admin/controllers/",
  "/admin/devices/",
  "/admin/client-errors/",
  "/admin/resources/",
  "/admin/sessions/",
  "/admin/statistics/",
  "/admin/activity/",
  "/admin/logs/",
  "/admin/jobs/",
  "/admin/rendering/",
  "/admin/bios/",
  "/admin/decorations/",
  "/admin/collections/",
  "/admin/attention/",
  "/admin/patches/",
  "/admin/cores/",
  "/admin/configuration/",
  "/admin/network/",
  "/admin/streaming/",
  "/admin/storage/",
  "/admin/backups/",
  "/admin/telemetry/",
  "/admin/about/",
  "/admin/appearance/",
] as const;

/** A screen's path, with its trailing slash. */
export type ScreenPath = (typeof SCREEN_PATHS)[number];
