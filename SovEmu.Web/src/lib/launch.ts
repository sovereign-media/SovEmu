// The loading bar while a game starts: the server's phases (saving the last session, starting
// the emulator, opening the core, reading the game) turned into one bar that only moves forward.
//
// Reading the game is most of a slow start. When the server knows how much it will read the bar
// is exact; when the core reads a disc itself it is not, and the bar eases towards the end of
// that phase instead (always moving, never arriving) while the megabytes read count up.

import type { Enum } from "./gql";
import type { Link } from "./protocol";

type StartPhase = Enum<"StartPhase">;

export type { StartPhase };

/** What the server says about a start (the `startProgress` query). */
export interface StartStatus {
  phase: StartPhase;
  read?: number | null;
  total?: number | null;
  elapsedMs: number;
  phaseMs: number;
  error?: string | null;
}

/** Each phase's share of the bar, and what it is called. */
const PHASES: Record<StartPhase, { from: number; to: number; label: string; ease: number }> = {
  STOPPING: { from: 0, to: 0.08, label: "Saving your last game", ease: 3000 },
  LAUNCHING: { from: 0.08, to: 0.12, label: "Starting the emulator", ease: 1000 },
  CORE: { from: 0.12, to: 0.18, label: "Loading the emulator", ease: 1500 },
  CONTENT: { from: 0.18, to: 0.85, label: "Loading the game", ease: 15000 },
  READY: { from: 0.85, to: 0.85, label: "Ready", ease: 1 },
  FAILED: { from: 0, to: 0, label: "Could not start", ease: 1 },
};

const MB = 1024 * 1024;
const mb = (bytes: number) => (bytes >= 100 * MB ? Math.round(bytes / MB) : Math.round((bytes / MB) * 10) / 10);

export interface LaunchView {
  label: string;
  /** Megabytes and seconds, or the error. */
  detail: string;
  /** 0..1 along the whole start. */
  fraction: number;
}

export function describeStart(s: StartStatus): LaunchView {
  const p = PHASES[s.phase];
  if (s.phase === "FAILED") return { label: p.label, detail: s.error ?? "", fraction: 0 };
  const read = s.read ?? null;
  const total = s.total ?? null;
  const within = total && total > 0 && read !== null ? Math.min(1, read / total) : 1 - Math.exp(-s.phaseMs / p.ease);
  const parts: string[] = [];
  if (s.phase === "CONTENT" && read !== null && read > 0) {
    parts.push(total ? `${mb(Math.min(read, total))} of ${mb(total)} MB` : `${mb(read)} MB read`);
  }
  const seconds = Math.floor(s.elapsedMs / 1000);
  if (seconds >= 3) parts.push(`${seconds} s`);
  return { label: p.label, detail: parts.join(" · "), fraction: p.from + (p.to - p.from) * within };
}

/** After this long without a picture the wait says how to get out of it. */
const NO_PICTURE_HINT_MS = 20_000;

/** Where the check of the connection takes the bar: from the session being ready to almost full. */
const NETWORK = { from: 0.85, to: 0.97 };

/**
 * The session is ready and this client is reaching it (ICE, then DTLS): nothing to show of
 * the game yet, and the worker's check of the connection comes once connected.
 */
export function describeConnecting(waitedMs: number, attempt = 0): LaunchView {
  const seconds = Math.floor(waitedMs / 1000);
  const parts: string[] = [];
  if (seconds >= 3) parts.push(`${seconds} s`);
  if (waitedMs >= NO_PICTURE_HINT_MS) parts.push("Escape opens the menu");
  return { label: attempt > 0 ? "Reconnecting" : "Connecting", detail: parts.join(" · "), fraction: NETWORK.from };
}

/**
 * Reconnecting (#302): a connection that fails, or stays disconnected past
 * {@link DISCONNECTED_GRACE_MS}, or never connects within {@link CONNECT_TIMEOUT_MS}, is offered
 * again to the same session, which takes the new client in place of the old. After
 * {@link RECONNECT_ATTEMPTS} the start has failed: {@link describeConnectionFailed}.
 */
export const RECONNECT_ATTEMPTS = 3;
export const DISCONNECTED_GRACE_MS = 5000;
export const CONNECT_TIMEOUT_MS = 15_000;

/** The loading screen once reconnecting has given up. */
export function describeConnectionFailed(): LaunchView {
  return {
    label: "Could not connect to the game",
    detail: "The server's game ports may be unreachable from here · Escape opens the menu",
    fraction: 0,
  };
}

/**
 * The check of the player's connection before the stream starts (`0x83`, #243): the worker
 * sends no video meanwhile, so it runs between the session being ready and the first picture.
 * The bar follows its time against its budget. Where the player is shows in a chip of its own
 * (`networkLabel`).
 */
export function describeLink(link: Link): LaunchView {
  const within = link.budgetMs > 0 ? Math.min(1, link.elapsedMs / link.budgetMs) : 1;
  return {
    label: "Checking your connection",
    detail: "",
    fraction: NETWORK.from + (NETWORK.to - NETWORK.from) * within,
  };
}

/**
 * The last stretch of a start, which the server cannot see: the session is ready (and the
 * connection checked), and the game has yet to draw its first picture (a console's BIOS boots
 * for seconds; a save is loaded once the game is up). The detail counts the wait.
 */
export function describeFirstPicture(waitedMs: number): LaunchView {
  const seconds = Math.floor(waitedMs / 1000);
  const parts: string[] = [];
  if (seconds >= 3) parts.push(`${seconds} s`);
  if (waitedMs >= NO_PICTURE_HINT_MS) parts.push("still no picture: Escape opens the menu");
  return { label: "Starting the game", detail: parts.join(" · "), fraction: NETWORK.to };
}

/** The loading screen's background: a picture, and how it is drawn. */
export interface LoadingBackground {
  url: string;
  /** A save's screenshot, at the console's own resolution: drawn with sharp pixels. */
  pixelated: boolean;
  /** Box art, which is not a picture of the game being played: blurred into a backdrop. */
  blurred: boolean;
}

/**
 * What the loading screen shows behind the start (#243), first found of: the screenshot of the
 * save being continued, the game's own screenshot, its box art (blurred). Null leaves the
 * system's colour.
 */
export function loadingBackground(
  saveShot: string | null | undefined,
  image: string | null | undefined,
  boxArt: string | null | undefined,
): LoadingBackground | null {
  if (saveShot) return { url: saveShot, pixelated: true, blurred: false };
  if (image) return { url: image, pixelated: false, blurred: false };
  if (boxArt) return { url: boxArt, pixelated: false, blurred: true };
  return null;
}

/** Keeps a bar from ever moving backwards (a phase can report less than the one before). */
export class ForwardOnly {
  private max = 0;
  next(fraction: number): number {
    this.max = Math.max(this.max, Math.min(1, Math.max(0, fraction)));
    return this.max;
  }
}

/** An id for following one start (letters, digits and dashes, as the server requires). */
export function newStartId(): string {
  return crypto.randomUUID();
}
