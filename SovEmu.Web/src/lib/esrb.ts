// The ESRB's age ratings, as the game panel shows them: SovEmu's own badge for each
// (`src/assets/esrb/`, the rating's code over its ages; not the ESRB's icons) and its name.
import ao from "../assets/esrb/ao.svg";
import e from "../assets/esrb/e.svg";
import e10 from "../assets/esrb/e10.svg";
import ec from "../assets/esrb/ec.svg";
import m from "../assets/esrb/m.svg";
import rp from "../assets/esrb/rp.svg";
import t from "../assets/esrb/t.svg";
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
