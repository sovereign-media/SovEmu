// The admin's sessions page in words: how long a game has run, how its stream is keeping up,
// and the player's view while their stats panel is open. Labels for the pipeline itself are
// `pipeline.ts`'s, shared with the stats overlay.
import type { ActiveSession, SessionFailure } from "./api";

/** `45 s`, `12 min`, `1 h 05 min`. */
export function durationLabel(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  if (s < 60) return `${s} s`;
  const minutes = Math.floor(s / 60);
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)} h ${String(minutes % 60).padStart(2, "0")} min`;
}

/**
 * Where a frame's time goes, per frame the core ran: `core 24.1 ms · GPU readback 6.3 ms ·
 * encode 1.8 ms` (no readback for a core that draws on the CPU).
 */
export function frameWorkLabel(h: NonNullable<ActiveSession["health"]>): string {
  const parts = [`core ${h.coreMeanMs.toFixed(1)} ms`];
  if (h.readbackMeanMs > 0) parts.push(`GPU readback ${h.readbackMeanMs.toFixed(1)} ms`);
  parts.push(`encode ${h.encodeMeanMs.toFixed(1)} ms`);
  return parts.join(" · ");
}

/** A session that runs late on more than this share of its frames is struggling. */
const STRUGGLING = 0.05;

export interface HealthSummary {
  /** `59.9 / 60 fps · 1,820 kbps · encode 1.2 ms`. */
  text: string;
  /** Frames ran late often enough to notice (stutter, choppy audio). */
  struggling: boolean;
}

/** The worker's last health interval against the game's own frame rate; null before the first. */
export function healthSummary(s: Pick<ActiveSession, "health" | "pipeline">): HealthSummary | null {
  const h = s.health;
  if (!h) return null;
  // The player's menu is open: the game is held, so nothing is drawn or sent.
  if (h.paused > 0.5) return { text: "Paused (menu open)", struggling: false };
  const target = s.pipeline?.fps;
  const fps = target ? `${h.fps.toFixed(1)} / ${Math.round(target)} fps` : `${h.fps.toFixed(1)} fps`;
  const kbps = `${Math.round(h.videoKbps).toLocaleString("en-US")} kbps`;
  return {
    text: `${fps} · ${kbps} · encode ${h.encodeMeanMs.toFixed(1)} ms`,
    struggling: h.overran > STRUGGLING,
  };
}

/**
 * The player's decoder and whether it runs in low-latency mode:
 * `c2.amlogic.avc.decoder · low latency: not supported`. Null while their panel is closed or
 * the client does not say.
 */
export function decoderSummary(s: Pick<ActiveSession, "clientStats">): string | null {
  const c = s.clientStats;
  if (!c?.decoder) return null;
  if (c.lowLatencyDecode == null) return c.decoder;
  return `${c.decoder} · low latency: ${c.lowLatencyDecode ? "on" : "not supported"}`;
}

/**
 * `39 ms button→frame · ~47 ms est. · decode 3.1 ms` (measured where the client measures, else
 * its estimate alone), or null while the player's stats panel is closed.
 */
export function clientSummary(s: Pick<ActiveSession, "clientStats">): string | null {
  const c = s.clientStats;
  if (!c) return null;
  const parts = [
    c.inputToFrameMs != null ? `${Math.round(c.inputToFrameMs)} ms button→frame` : null,
    c.estimateMs != null
      ? c.inputToFrameMs != null
        ? `~${Math.round(c.estimateMs)} ms est.`
        : `~${Math.round(c.estimateMs)} ms button→pixel`
      : null,
    c.decodeMs != null ? `decode ${c.decodeMs.toFixed(1)} ms` : null,
    c.packetsLost ? `${c.packetsLost} lost` : null,
  ].filter(Boolean);
  return parts.length ? parts.join(" · ") : null;
}

const SIGNALS: Record<number, string> = {
  4: "SIGILL",
  6: "SIGABRT",
  7: "SIGBUS",
  8: "SIGFPE",
  9: "SIGKILL",
  11: "SIGSEGV",
};

/** How a game failed, in words: `Crashed while playing (SIGSEGV)`, `Failed to start (exit 1)`. */
export function failureLabel(f: Pick<SessionFailure, "whilePlaying" | "reason" | "exitCode" | "signal">): string {
  let what: string;
  if (f.reason === "killed") what = f.whilePlaying ? "Would not stop" : "Timed out starting";
  else if (f.reason === "crashed") what = f.whilePlaying ? "Crashed while playing" : "Crashed starting";
  else what = f.whilePlaying ? "Failed while playing" : "Failed to start";
  if (f.signal != null) return `${what} (${SIGNALS[f.signal] ?? `signal ${f.signal}`})`;
  if (f.exitCode != null) return `${what} (exit ${f.exitCode})`;
  return what;
}
