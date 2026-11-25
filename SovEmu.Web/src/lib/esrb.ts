// The ESRB's age ratings, as the game panel names them. Their icons come from the server
// (`Game.esrbBadgeUrl`: `artwork/ratings/esrb/`, built in, which an admin may replace).
import type { EsrbRating } from "./api";

export interface EsrbInfo {
  /** As the ESRB writes it: "E10+". */
  code: string;
  /** Its name: "Everyone 10+". */
  name: string;
}

export const ESRB: Record<EsrbRating, EsrbInfo> = {
  EARLY_CHILDHOOD: { code: "EC", name: "Early Childhood" },
  EVERYONE: { code: "E", name: "Everyone" },
  EVERYONE_10_PLUS: { code: "E10+", name: "Everyone 10+" },
  TEEN: { code: "T", name: "Teen" },
  MATURE: { code: "M", name: "Mature 17+" },
  ADULTS_ONLY: { code: "AO", name: "Adults Only 18+" },
  RATING_PENDING: { code: "RP", name: "Rating Pending" },
};

/** The ratings from the youngest audience to the oldest; pending last. */
export const ESRB_ORDER: EsrbRating[] = [
  "EARLY_CHILDHOOD",
  "EVERYONE",
  "EVERYONE_10_PLUS",
  "TEEN",
  "MATURE",
  "ADULTS_ONLY",
  "RATING_PENDING",
];

/** What a badge says to a screen reader and in its tooltip: "ESRB: Teen (T)". */
export const esrbLabel = (rating: EsrbRating): string => `ESRB: ${ESRB[rating].name} (${ESRB[rating].code})`;

/** A rating out of 1 as stars out of 5, to the half: 0.8 → 4. */
export const stars = (rating: number): number => Math.round(Math.min(1, Math.max(0, rating)) * 10) / 2;
