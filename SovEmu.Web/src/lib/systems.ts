import type { SystemSummary } from "./api";

/** The small status badge on a system's card, if it needs one. */
export function systemBadge(s: SystemSummary): string | null {
  if (s.needs_gpu && s.rendering === "off") return "no 3D rendering";
  if (!s.playable) return "no core";
  if (s.needs_gpu && s.rendering === "software") return "software rendering";
  return null;
}
