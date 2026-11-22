// How the stream is made, in words: the server's side from its pipeline message, the browser's
// decoder from getStats. The stats overlay shows these rows, the admin's sessions page the same
// names; the Android app formats them identically.
import type { Pipeline } from "./protocol";
import type { StatItem } from "./stats";

const CODECS: Record<string, string> = { h264: "H.264", vp8: "VP8", opus: "Opus" };
const ENCODERS: Record<string, string> = { vaapi: "VA-API", nvenc: "NVENC", x264: "x264", libvpx: "libvpx" };

/** `h264` → `H.264`; an unknown name as it came. */
export const codecName = (codec: string) => CODECS[codec] ?? codec;
/** `vaapi` → `VA-API`; an unknown name as it came. */
export const encoderName = (encoder: string) => ENCODERS[encoder] ?? encoder;

/** `H.264 · VA-API (GPU)`. */
export function streamLabel(codec: string, encoder: string, hardware: boolean): string {
  return `${codecName(codec)} · ${encoderName(encoder)} (${hardware ? "GPU" : "CPU"})`;
}

/** `GPU · Mesa Intel(R) …`, `Software · llvmpipe …`, or `CPU` for a core that draws itself. */
export function renderingLabel(renderer: string | null | undefined, software: boolean): string {
  if (!renderer) return "CPU";
  return `${software ? "Software" : "GPU"} · ${renderer}`;
}

/** `Off`, `1 frame`, `2 frames`. */
export const runAheadLabel = (frames: number) => (frames === 0 ? "Off" : frames === 1 ? "1 frame" : `${frames} frames`);

/** `Opus 128 kbps · from 32,040 Hz`. */
export function audioLabel(codec: string, kbps: number, sourceHz: number): string {
  return `${codecName(codec)} ${kbps} kbps · from ${Math.round(sourceHz).toLocaleString("en-US")} Hz`;
}

/** `VA-API: no device; NVENC: not in this libavcodec`. */
export const fallbacksLabel = (fallbacks: { encoder: string; reason: string }[]) =>
  fallbacks.map((f) => `${encoderName(f.encoder)}: ${f.reason}`).join("; ");

/** A change in the stream's quality, as the player is told of it: one line, and the glyph before it. */
export interface QualityNotice {
  line: string;
  /** `wifi-alert` and `wifi-up` (Sovereign's own), or `gauge` for a device that fell behind. */
  icon: "wifi-alert" | "wifi-up" | "gauge";
}

/**
 * The notice for a change in the stream's quality between two pipeline messages, worded and
 * marked as Sovereign's step-down and step-up notices: the bitrate the worker tells the player
 * of moved (it only does in steps worth telling). Null when it has not, or when either side
 * does not know it (the first message, an older server).
 */
export function qualityNotice(before: Pipeline | null, now: Pipeline): QualityNotice | null {
  const [was, is] = [before?.video.kbps ?? 0, now.video.kbps];
  if (!was || !is || was === is) return null;
  if (is < was) {
    return now.video.limitedBy === "client"
      ? { line: "This device is falling behind. Playing at a lower quality.", icon: "gauge" }
      : { line: "Slow connection. Playing at a lower quality.", icon: "wifi-alert" };
  }
  // Back up from a fall that was the device's: the connection had nothing to do with it.
  return before?.video.limitedBy === "client"
    ? { line: "Playing at a higher quality.", icon: "gauge" }
    : { line: "Connection improved. Playing at a higher quality.", icon: "wifi-up" };
}

/** The overlay's rows for the server's side of the stream. */
export function pipelineItems(p: Pipeline): StatItem[] {
  const items: StatItem[] = [{ label: "Stream", value: streamLabel(p.video.codec, p.video.encoder, p.video.hardware) }];
  if (p.video.fallbacks.length) items.push({ label: "Fell back from", value: fallbacksLabel(p.video.fallbacks) });
  items.push(
    { label: "Rendering", value: renderingLabel(p.renderer, p.rendererSoftware) },
    { label: "Audio", value: audioLabel(p.audio.codec, p.audio.kbps, p.audio.sourceHz) },
    { label: "Run-ahead", value: runAheadLabel(p.runAhead) },
  );
  return items;
}

/**
 * The browser's video decoder, from the inbound video stats: `ExternalDecoder (hardware)`. Null
 * where the browser does not say (Firefox, Safari).
 */
export function decoderLabel(implementation: unknown, powerEfficient: unknown): string | null {
  if (typeof implementation !== "string" || !implementation) return null;
  if (typeof powerEfficient !== "boolean") return implementation;
  return `${implementation} (${powerEfficient ? "hardware" : "software"})`;
}
