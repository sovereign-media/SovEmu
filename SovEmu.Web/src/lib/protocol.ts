// The input channel's wire format, mirroring SovEmu.Protocol (src/input.rs) byte for byte.
// protocol.test.ts checks these against the same vectors as the Rust tests; change both together.

export const INPUT_VERSION = 1;
export const INPUT_CHANNEL = "input";
export const MAX_PORTS = 4;

const TYPE_INPUT = 0x01;
const TYPE_HOTKEYS = 0x02;
const TYPE_ANALOG = 0x03;
const TYPE_STATS = 0x04;
const TYPE_ACK = 0x81;
const TYPE_PIPELINE = 0x82;

/** RetroPad button ids (libretro RETRO_DEVICE_ID_JOYPAD_*): bit n of a port's state. */
export const Button = {
  B: 0,
  Y: 1,
  SELECT: 2,
  START: 3,
  UP: 4,
  DOWN: 5,
  LEFT: 6,
  RIGHT: 7,
  A: 8,
  X: 9,
  L: 10,
  R: 11,
  L2: 12,
  R2: 13,
  L3: 14,
  R3: 15,
} as const;
export type ButtonName = keyof typeof Button;

export const bit = (name: ButtonName): number => 1 << Button[name];

export interface InputPacket {
  seq: number;
  clientTimeMs: number;
  /** Button state per port, ports 0..n. */
  ports: number[];
}

export interface InputAck {
  seq: number;
  clientTimeMs: number;
  /** The emulated frame that first saw the input. */
  frame: number;
}

export function encodeInput(packet: InputPacket): ArrayBuffer {
  const ports = packet.ports.slice(0, MAX_PORTS);
  const buf = new ArrayBuffer(11 + 2 * ports.length);
  const view = new DataView(buf);
  view.setUint8(0, TYPE_INPUT);
  view.setUint8(1, INPUT_VERSION);
  view.setUint32(2, packet.seq >>> 0, true);
  view.setUint32(6, packet.clientTimeMs >>> 0, true);
  view.setUint8(10, ports.length);
  ports.forEach((state, i) => {
    view.setUint16(11 + 2 * i, state & 0xffff, true);
  });
  return buf;
}

export interface Hotkeys {
  fastForward: boolean;
  rewind: boolean;
  /** The game is paused (the in-game menu is open): the worker runs no frames. */
  pause?: boolean;
}

/** The hotkeys flags byte: bit 0 fast-forward, bit 1 rewind, bit 2 pause. */
export const hotkeyBits = (h: Hotkeys): number => (h.fastForward ? 1 : 0) | (h.rewind ? 2 : 0) | (h.pause ? 4 : 0);

export function encodeHotkeys(h: Hotkeys): ArrayBuffer {
  return new Uint8Array([TYPE_HOTKEYS, INPUT_VERSION, hotkeyBits(h)]).buffer;
}

/** One port's analog controls: sticks −32768..32767 (x right, y down), triggers 0..32767. */
export interface Analog {
  lx: number;
  ly: number;
  rx: number;
  ry: number;
  l2: number;
  r2: number;
}

export const CENTRED: Analog = { lx: 0, ly: 0, rx: 0, ry: 0, l2: 0, r2: 0 };

export const analogEqual = (a: Analog, b: Analog) =>
  a.lx === b.lx && a.ly === b.ly && a.rx === b.rx && a.ry === b.ry && a.l2 === b.l2 && a.r2 === b.r2;

export function encodeAnalog(packet: { seq: number; ports: Analog[] }): ArrayBuffer {
  const ports = packet.ports.slice(0, MAX_PORTS);
  const buf = new ArrayBuffer(7 + 12 * ports.length);
  const view = new DataView(buf);
  view.setUint8(0, TYPE_ANALOG);
  view.setUint8(1, INPUT_VERSION);
  view.setUint32(2, packet.seq >>> 0, true);
  view.setUint8(6, ports.length);
  ports.forEach((a, i) => {
    [a.lx, a.ly, a.rx, a.ry, a.l2, a.r2].forEach((v, j) => {
      view.setInt16(7 + 12 * i + 2 * j, Math.max(-32768, Math.min(32767, Math.round(v))), true);
    });
  });
  return buf;
}

/**
 * The client's view of the stream, sent once a second while playing (the stats overlay's
 * numbers, whether or not it is open): the worker lowers the video bitrate for a client that
 * is not keeping up (`fps`, `frames_dropped`, `decode_ms`) and logs the client's view beside
 * its own (`client stats`). Times in ms, rates in kbit/s; every key is optional. The keys and their order are `ClientStats` in SovEmu.Protocol.
 */
export interface ClientStats {
  client?: string;
  estimate_ms?: number;
  /** Button to frame, measured (`latency.ts`). */
  input_to_frame_ms?: number;
  input_rtt_p50_ms?: number;
  input_rtt_p95_ms?: number;
  net_rtt_ms?: number;
  jitter_ms?: number;
  decode_ms?: number;
  width?: number;
  height?: number;
  fps?: number;
  kbps?: number;
  packets_lost?: number;
  /** Video frames dropped since the stream began (getStats' `framesDropped`). */
  frames_dropped?: number;
  game_mode?: boolean;
  wifi_lock?: boolean;
  low_latency_decode?: boolean;
  /** The video decoder (getStats' `decoderImplementation`). */
  decoder?: string;
}

const STATS_KEYS: (keyof ClientStats)[] = [
  "client",
  "estimate_ms",
  "input_to_frame_ms",
  "input_rtt_p50_ms",
  "input_rtt_p95_ms",
  "net_rtt_ms",
  "jitter_ms",
  "decode_ms",
  "width",
  "height",
  "fps",
  "kbps",
  "packets_lost",
  "frames_dropped",
  "game_mode",
  "wifi_lock",
  "low_latency_decode",
  "decoder",
];

/** `0x04`, the version, then the known values as a JSON object (unknown and non-finite ones left out). */
export function encodeStats(stats: ClientStats): ArrayBuffer {
  const known: Record<string, unknown> = {};
  for (const key of STATS_KEYS) {
    const v = stats[key];
    if (v === undefined || v === null || (typeof v === "number" && !Number.isFinite(v))) continue;
    known[key] = typeof v === "number" ? Math.round(v * 10) / 10 : v;
  }
  const body = new TextEncoder().encode(JSON.stringify(known));
  const out = new Uint8Array(2 + body.length);
  out.set([TYPE_STATS, INPUT_VERSION]);
  out.set(body, 2);
  return out.buffer;
}

/** Decodes an ack, or returns null for anything else. */
export function decodeAck(data: ArrayBuffer): InputAck | null {
  if (data.byteLength < 14) return null;
  const view = new DataView(data);
  if (view.getUint8(0) !== TYPE_ACK || view.getUint8(1) !== INPUT_VERSION) return null;
  return {
    seq: view.getUint32(2, true),
    clientTimeMs: view.getUint32(6, true),
    frame: view.getUint32(10, true),
  };
}

/**
 * How the worker makes the stream (`0x82`, then the version, then JSON): what draws the picture,
 * the video codec and encoder (and any it fell back from, with why), the audio, and run-ahead.
 * Sent when the channel opens, when anything changes, and every 2 s. `Pipeline` in
 * SovEmu.Protocol; every key may be missing.
 */
export interface Pipeline {
  /** The OpenGL renderer a 3D core draws with; null for a core that draws on the CPU. */
  renderer: string | null;
  /** The renderer is the software one (llvmpipe). */
  rendererSoftware: boolean;
  video: {
    codec: string;
    encoder: string;
    hardware: boolean;
    fallbacks: { encoder: string; reason: string }[];
    width: number;
    height: number;
    fps: number;
    /**
     * The video bitrate as last told to the player, kbit/s: it moves in steps worth a notice,
     * so a change from the one before is shown (`qualityNotice`). 0: unknown (an older server).
     */
    kbps: number;
    /** What held it down when it last fell: `connection`, `client`, or empty. */
    limitedBy: string;
  };
  audio: { codec: string; sourceHz: number; kbps: number };
  runAhead: number;
}

const str = (v: unknown) => (typeof v === "string" ? v : "");
const num = (v: unknown) => (typeof v === "number" && Number.isFinite(v) ? v : 0);
const obj = (v: unknown): Record<string, unknown> =>
  v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : {};

/** Decodes a pipeline message, or returns null for anything else. */
export function decodePipeline(data: ArrayBuffer): Pipeline | null {
  if (data.byteLength < 3 || data.byteLength > 4096) return null;
  const bytes = new Uint8Array(data);
  if (bytes[0] !== TYPE_PIPELINE || bytes[1] !== INPUT_VERSION) return null;
  let json: unknown;
  try {
    json = JSON.parse(new TextDecoder().decode(bytes.subarray(2)));
  } catch {
    return null;
  }
  if (!json || typeof json !== "object" || Array.isArray(json)) return null;
  const p = json as Record<string, unknown>;
  const video = obj(p.video);
  const audio = obj(p.audio);
  return {
    renderer: typeof p.renderer === "string" ? p.renderer : null,
    rendererSoftware: p.renderer_software === true,
    video: {
      codec: str(video.codec),
      encoder: str(video.encoder),
      hardware: video.hardware === true,
      fallbacks: (Array.isArray(video.fallbacks) ? video.fallbacks : []).map((f) => ({
        encoder: str(obj(f).encoder),
        reason: str(obj(f).reason),
      })),
      width: num(video.width),
      height: num(video.height),
      fps: num(video.fps),
      kbps: num(video.kbps),
      limitedBy: str(video.limited_by),
    },
    audio: { codec: str(audio.codec), sourceHz: num(audio.source_hz), kbps: num(audio.kbps) },
    runAhead: num(p.run_ahead),
  };
}
