// What goes wrong in the browser, for the server's log (`reportClientEvents`, logged as
// `client event` beside the game's own lines). Events are batched for a moment and sent only
// while signed in; a report that fails is dropped, never retried, and never shown.
import { quietly } from "./graphql";
import { ReportClientEventsMutation } from "./queries";
import type { ClientEventKind } from "../graphql/graphql";

export interface ClientEvent {
  kind: ClientEventKind;
  message: string;
  sessionId?: string | null;
  /** Seconds since the Unix epoch. */
  at: number;
}

/** The server takes at most this many a call. */
export const MAX_BATCH = 20;
/** How long events gather before they are sent. */
const GATHER_MS = 2000;

/** "web", and the browser: `web (Chrome 131, Linux)`. */
export function clientName(userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent): string {
  const browser = /(Edg|Firefox|Chrome|Safari)\/(\d+)/.exec(userAgent);
  const os = /(Windows|Android|iPhone|iPad|Mac OS X|Linux|CrOS)/.exec(userAgent)?.[1];
  // Safari's own version is in `Version/`; its `Safari/` is WebKit's build.
  const version = browser?.[1] === "Safari" ? (/Version\/(\d+)/.exec(userAgent)?.[1] ?? browser[2]) : browser?.[2];
  const name = browser ? `${browser[1] === "Edg" ? "Edge" : browser[1]} ${version}` : null;
  return `web${name || os ? ` (${[name, os].filter(Boolean).join(", ")})` : ""}`;
}

/** Gathers events and sends them in batches. */
export class Reporter {
  private queue: ClientEvent[] = [];
  private timer: ReturnType<typeof setTimeout> | null = null;
  /** Messages already sent, so a fault repeating every frame is sent once. */
  private seen = new Set<string>();

  constructor(
    private send: (events: ClientEvent[]) => void,
    private now = () => Date.now() / 1000,
  ) {}

  report(kind: ClientEventKind, message: string, sessionId?: string | null) {
    const key = `${kind}|${message}|${sessionId ?? ""}`;
    if (this.seen.has(key) || this.seen.size > 200) return;
    this.seen.add(key);
    this.queue.push({ kind, message: message.slice(0, 500), sessionId: sessionId ?? null, at: this.now() });
    if (this.queue.length >= MAX_BATCH) this.flush();
    else this.timer ??= setTimeout(() => this.flush(), GATHER_MS);
  }

  flush() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    while (this.queue.length) this.send(this.queue.splice(0, MAX_BATCH));
  }
}

const reporter = new Reporter((events) => quietly(ReportClientEventsMutation, { client: clientName(), events }));

/** Reports something that went wrong; see `ClientEventKind`. */
export const reportClientEvent = (kind: ClientEventKind, message: string, sessionId?: string | null) =>
  reporter.report(kind, message, sessionId);

let installed = false;

/** Reports the page's uncaught errors and rejections as `SCREEN_ERROR`, once per page. */
export function reportUncaughtErrors() {
  if (installed || typeof window === "undefined") return;
  installed = true;
  addEventListener("error", (e) => reportClientEvent("SCREEN_ERROR", `${e.message} (${e.filename}:${e.lineno})`));
  addEventListener("unhandledrejection", (e) => {
    const reason = e.reason instanceof Error ? `${e.reason.name}: ${e.reason.message}` : String(e.reason);
    reportClientEvent("SCREEN_ERROR", `unhandled rejection: ${reason}`);
  });
  addEventListener("pagehide", () => reporter.flush());
}
