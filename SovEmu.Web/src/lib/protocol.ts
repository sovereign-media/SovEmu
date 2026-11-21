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
 * What the stats overlay shows, sent once a second while it is open so the worker logs the
 * client's view beside its own (`client stats`). Times in ms, rates in kbit/s; every key is
 * optional. The keys and their order are `ClientStats` in SovEmu.Protocol.
 */
export interface ClientStats {
  client?: string;
  estimate_ms?: number;
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
  game_mode?: boolean;
  wifi_lock?: boolean;
}

const STATS_KEYS: (keyof ClientStats)[] = [
  "client",
  "estimate_ms",
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
  "game_mode",
  "wifi_lock",
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
