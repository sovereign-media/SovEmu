// Signing a TV in from the browser (`/link/`, where a TV's *Sign in with your phone* screen sends
// people): the person types the code the TV shows, and the page approves it as them with the
// `approveDeviceCode` mutation, as the phone apps' *Sign in a TV* does. The words and the code's
// handling are the apps' (SovEmu.Android's `TvSignIn`), so the three read alike.
import { ApiError } from "./graphql";

/** The code's characters, without its dash. */
export const CODE_LENGTH = 8;
const GROUP = CODE_LENGTH / 2;

/** The code's characters: letters and digits only, upper case, at most eight. The server
 *  matches in any case and with or without the dash, but a phone keyboard's lower case or a
 *  pasted space would make the field look wrong. */
export function normalizeCode(input: string): string {
  return input
    .replace(/[^0-9a-z]/gi, "")
    .toUpperCase()
    .slice(0, CODE_LENGTH);
}

/** The code as the TV shows it (`QG3W-RY4W`), the dash after the fourth character. */
export function formatCode(input: string): string {
  const code = normalizeCode(input);
  return code.length > GROUP ? `${code.slice(0, GROUP)}-${code.slice(GROUP)}` : code;
}

/** Whether all eight characters are there: the Approve button's gate. */
export const isCompleteCode = (input: string) => normalizeCode(input).length === CODE_LENGTH;

/** What the page says when the approval failed. */
export function failureMessage(error: unknown): string {
  switch (error instanceof ApiError ? error.code : null) {
    case "NOT_FOUND":
      return "No TV is waiting with that code. Check the code, and that the TV is connected to this server.";
    case "TOO_MANY_REQUESTS":
      return "Too many codes just now. Try again in a minute.";
    default:
      return error instanceof Error && error.message ? error.message : "The TV could not be signed in.";
  }
}
