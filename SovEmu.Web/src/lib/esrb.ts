// The ESRB's age ratings, as the game panel shows them: the ESRB's own icon for each and its
// name. The icons are WebP images shipped with the client (`src/assets/esrb/`), rendered from
// the 2013 set's SVGs (Wikimedia Commons, `scripts/esrb-badges/`) by `npm run esrb-badges`; the
// apps ship the same files.
import ao from "../assets/esrb/esrb_ao.webp";
import e from "../assets/esrb/esrb_e.webp";
import e10 from "../assets/esrb/esrb_e10.webp";
import ec from "../assets/esrb/esrb_ec.webp";
import m from "../assets/esrb/esrb_m.webp";
import rp from "../assets/esrb/esrb_rp.webp";
import t from "../assets/esrb/esrb_t.webp";
import type { EsrbRating } from "./api";

export interface EsrbInfo {
  /** As the ESRB writes it: "E10+". */
  code: string;
  /** Its name: "Everyone 10+". */
  name: string;
  /** The badge's URL. */
  badge: string;
}

export const ESRB: Record<EsrbRating, EsrbInfo> = {
  EARLY_CHILDHOOD: { code: "EC", name: "Early Childhood", badge: ec },
  EVERYONE: { code: "E", name: "Everyone", badge: e },
  EVERYONE_10_PLUS: { code: "E10+", name: "Everyone 10+", badge: e10 },
  TEEN: { code: "T", name: "Teen", badge: t },
  MATURE: { code: "M", name: "Mature 17+", badge: m },
  ADULTS_ONLY: { code: "AO", name: "Adults Only 18+", badge: ao },
  RATING_PENDING: { code: "RP", name: "Rating Pending", badge: rp },
};

/** What a badge says to a screen reader and in its tooltip: "ESRB: Teen (T)". */
export const esrbLabel = (rating: EsrbRating): string => `ESRB: ${ESRB[rating].name} (${ESRB[rating].code})`;

/** A rating out of 1 as stars out of 5, to the half: 0.8 → 4. */
export const stars = (rating: number): number => Math.round(Math.min(1, Math.max(0, rating)) * 10) / 2;
