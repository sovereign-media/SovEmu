// What this browser can decode and show, as the server takes it with every offer
// (`sessionOffer`'s `capabilities`): one complete document of this version, sent whole each
// time. The server keeps it against this sign-in and prices each video codec for this device
// by it, until the decoder's own measured times (the stats sent once a second) replace it.
//
// A browser says less than an app can. For each codec the page asks the Media Capabilities API
// about a WebRTC stream at a console's size: `supported` is whether there is a row at all, and
// `powerEfficient` is the browser's word for "decoded by hardware". Browsers name no decoder
// and have no low-latency switch, but their WebRTC decoders hand over each frame as it is
// decoded, so every row says low latency.

import type { VariablesOf } from "./gql";
import type { SessionOfferMutation } from "./queries";

export type Capabilities = NonNullable<VariablesOf<typeof SessionOfferMutation>["capabilities"]>;
type VideoCodec = Capabilities["videoDecoders"][number]["codec"];

/** The document's shape. The server refuses any other, asking the player to update. */
export const CAPABILITIES_VERSION = 1;

/** Each codec the server names, and the WebRTC content type that asks a browser about it. */
const CODECS: [VideoCodec, string][] = [
  ["VP8", "video/VP8"],
  ["H264", "video/H264;level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"],
  ["VP9", "video/VP9"],
  ["AV1", "video/AV1"],
  ["H265", "video/H265"],
];

/** What a browser answers about decoding one kind of stream. */
export interface DecodeAnswer {
  supported: boolean;
  powerEfficient: boolean;
}

/** Asks about one WebRTC content type; rejects where the browser cannot be asked. */
export type AskDecoder = (contentType: string) => Promise<DecodeAnswer>;

/** A screen: CSS pixels, the device pixels in each, and pictures a second. */
export interface Screen {
  width: number;
  height: number;
  pixelRatio: number;
  refreshHz: number;
}

/** A row for each codec the browser says it decodes; a codec it cannot be asked about has none. */
export async function videoDecoders(ask: AskDecoder): Promise<Capabilities["videoDecoders"]> {
  const rows = await Promise.all(
    CODECS.map(async ([codec, contentType]) => {
      try {
        const answer = await ask(contentType);
        return answer.supported ? { codec, hardware: answer.powerEfficient, lowLatency: true, name: null } : null;
      } catch {
        return null;
      }
    }),
  );
  return rows.filter((row) => row !== null);
}

/** The refresh rates screens come in: a measured rate within 3% of one is that one. */
const COMMON_RATES = [24, 30, 48, 50, 60, 72, 75, 90, 100, 120, 144, 165, 240, 360];

/**
 * The refresh rate behind the gaps between animation frames (ms): the median gap's rate, as
 * the common rate it is nearest (frame times are only good to a fraction of a millisecond),
 * or 60 where there are too few gaps to say (a hidden tab draws none).
 */
export function refreshRate(gapsMs: number[]): number {
  const gaps = gapsMs.filter((gap) => gap > 0).sort((a, b) => a - b);
  if (gaps.length < 5) return 60;
  const hz = 1000 / gaps[Math.floor(gaps.length / 2)];
  const common = COMMON_RATES.find((rate) => Math.abs(hz - rate) / rate <= 0.03);
  return common ?? Math.min(Math.max(Math.round(hz), 1), 1000);
}

/** The whole document. With no decoder to name (a browser that cannot be asked), VP8 on the CPU. */
export function document(decoders: Capabilities["videoDecoders"], screen: Screen): Capabilities {
  return {
    version: CAPABILITIES_VERSION,
    videoDecoders: decoders.length ? decoders : [{ codec: "VP8", hardware: false, lowLatency: true, name: null }],
    display: {
      width: Math.max(1, Math.round(screen.width * screen.pixelRatio)),
      height: Math.max(1, Math.round(screen.height * screen.pixelRatio)),
      refreshHz: screen.refreshHz,
    },
  };
}

/** Asks this browser's Media Capabilities about a WebRTC stream at a console's size. */
const askBrowser: AskDecoder = async (contentType) => {
  const info = await navigator.mediaCapabilities.decodingInfo({
    type: "webrtc",
    video: { contentType, width: 640, height: 480, bitrate: 3_000_000, framerate: 60 },
  });
  return { supported: info.supported, powerEfficient: info.powerEfficient };
};

/** The gaps between the next `frames` animation frames, or fewer if they stop coming. */
function frameGaps(frames: number, timeoutMs: number): Promise<number[]> {
  return new Promise((resolve) => {
    const gaps: number[] = [];
    let last: number | null = null;
    const done = () => resolve(gaps);
    const timer = setTimeout(done, timeoutMs);
    const tick = (now: number) => {
      if (last !== null) gaps.push(now - last);
      last = now;
      if (gaps.length >= frames) {
        clearTimeout(timer);
        done();
      } else {
        requestAnimationFrame(tick);
      }
    };
    requestAnimationFrame(tick);
  });
}

let decoders: Promise<Capabilities["videoDecoders"]> | null = null;

/**
 * This browser's capabilities, for an offer. The decoders are asked once a page (they do not
 * change); the screen is read each time (a window moves between screens).
 */
export async function clientCapabilities(): Promise<Capabilities> {
  decoders ??= videoDecoders(askBrowser);
  const [rows, gaps] = await Promise.all([decoders, frameGaps(20, 500)]);
  return document(rows, {
    width: screen.width,
    height: screen.height,
    pixelRatio: window.devicePixelRatio || 1,
    refreshHz: refreshRate(gaps),
  });
}
