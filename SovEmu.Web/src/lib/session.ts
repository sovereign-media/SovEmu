// One WebRTC connection to a game session: receive-only video and audio, plus the unordered,
// no-retransmit input channel. Signalling is one exchange of the complete offer (non-trickle) for
// the worker's answer: the `sessionOffer` mutation, passed in as `signal`.

import type { InputState } from "./input";
import { LatencyMeter } from "./latency";
import { supersedes } from "./link";
import { type QualityNotice, qualityNotice } from "./pipeline";
import {
  type Analog,
  analogEqual,
  CENTRED,
  type ClientStats,
  decodeAck,
  decodeLink,
  decodePipeline,
  encodeAnalog,
  encodeHotkeys,
  encodeInput,
  encodeStats,
  hotkeyBits,
  INPUT_CHANNEL,
  type Link,
  type Pipeline,
} from "./protocol";
import { startTicker } from "./ticker";

/** Send at least this often even when nothing changed: the channel drops lost packets. */
const HEARTBEAT_MS = 100;
const ICE_GATHER_TIMEOUT_MS = 1500;

/**
 * Resolves once `pc` has gathered its ICE candidates (or after a moment, with what it has): the
 * server answers an offer once, so the offer must carry them all (no trickle).
 */
export function iceGathered(pc: RTCPeerConnection): Promise<void> {
  if (pc.iceGatheringState === "complete") return Promise.resolve();
  return new Promise((resolve) => {
    pc.addEventListener("icegatheringstatechange", () => {
      if (pc.iceGatheringState === "complete") resolve();
    });
    setTimeout(resolve, ICE_GATHER_TIMEOUT_MS);
  });
}

export type SessionState = RTCPeerConnectionState | "idle";

export interface SessionEvents {
  onState?: (state: SessionState) => void;
  /** The worker changed the stream's quality: the notice to show (`qualityNotice`). */
  onQuality?: (notice: QualityNotice) => void;
  /** How the check of the connection before the stream starts goes (`0x83`, #243). */
  onLink?: (link: Link) => void;
}

export interface SessionOptions {
  /** STUN/TURN servers from the server's configuration (`clientConfig.iceServers`). */
  iceServers?: RTCIceServer[];
}

export class Session {
  readonly pc: RTCPeerConnection;
  /** Recent input round trips (send -> server applies on a frame -> ack), in ms. */
  readonly inputRtts: number[] = [];
  /** Button to frame, measured from acks and the frames shown after them. */
  readonly latency = new LatencyMeter();
  /** How the worker makes the stream, as it last said (null until it has). */
  pipeline: Pipeline | null = null;
  /** The check of the connection before the stream starts, as the worker last said (null
   *  until it has: an older worker never does). */
  link: Link | null = null;
  private channel: RTCDataChannel;
  private seq = 0;
  /** The last sent buttons, every player's, as a comparable key. */
  private sent = "";
  private sentHotkeys = 0;
  private lastSend = 0;
  private analogSeq = 0;
  private sentAnalog: Analog[] = [];
  private lastAnalogSend = 0;
  private raf = 0;
  private frameCallback = 0;
  private stopTicker: () => void = () => {};

  /** While true, the controller state is withheld (a menu is open): nothing is pressed. */
  suspended = false;

  constructor(
    /** Sends the offer (SDP) and returns the answer (SDP). */
    private readonly signal: (offer: string) => Promise<string>,
    private readonly video: HTMLVideoElement,
    private readonly input: InputState,
    private readonly events: SessionEvents = {},
    options: SessionOptions = {},
  ) {
    // One transport for everything (str0m answers bundled only): gathering, and any TURN
    // allocation, happens once rather than per media section, inside the non-trickle wait.
    this.pc = new RTCPeerConnection({ iceServers: options.iceServers ?? [], bundlePolicy: "max-bundle" });
    this.pc.addTransceiver("video", { direction: "recvonly" });
    this.pc.addTransceiver("audio", { direction: "recvonly" });
    this.channel = this.pc.createDataChannel(INPUT_CHANNEL, { ordered: false, maxRetransmits: 0 });
    this.channel.binaryType = "arraybuffer";
    this.channel.onmessage = (e) => this.onMessage(e.data as ArrayBuffer);

    const stream = new MediaStream();
    video.srcObject = stream;
    // Each frame as it reaches the screen (its presentation time, on performance.now()'s clock
    // like the inputs' sent times), for the measured latency. Browsers without the callback
    // (older Firefox) just measure nothing.
    if (video.requestVideoFrameCallback) {
      const shown: VideoFrameRequestCallback = (now, frame) => {
        this.latency.frameShown(frame.presentationTime ?? now, frame.receiveTime);
        this.frameCallback = video.requestVideoFrameCallback(shown);
      };
      this.frameCallback = video.requestVideoFrameCallback(shown);
    }
    this.pc.ontrack = (e) => {
      // Ask for the smallest jitter buffer the browser allows: latency over smoothness.
      const receiver = e.receiver as RTCRtpReceiver & { jitterBufferTarget?: number; playoutDelayHint?: number };
      try {
        receiver.jitterBufferTarget = 0;
      } catch {
        /* unsupported */
      }
      try {
        receiver.playoutDelayHint = 0;
      } catch {
        /* unsupported */
      }
      stream.addTrack(e.track);
    };
    this.pc.onconnectionstatechange = () => this.events.onState?.(this.pc.connectionState);
  }

  async connect(): Promise<void> {
    await this.pc.setLocalDescription(await this.pc.createOffer());
    await iceGathered(this.pc);
    const answer = await this.signal(this.pc.localDescription?.sdp ?? "");
    await this.pc.setRemoteDescription({ type: "answer", sdp: answer });
    this.video.play().catch(() => {
      /* needs a user gesture; Connect is one */
    });
    this.raf = requestAnimationFrame(this.pump);
    this.stopTicker = startTicker(HEARTBEAT_MS, this.hiddenTick);
  }

  close(): void {
    cancelAnimationFrame(this.raf);
    this.stopTicker();
    this.video.cancelVideoFrameCallback?.(this.frameCallback);
    this.pc.close();
  }

  /** Sends this client's view of the stream to the worker (see `encodeStats`). */
  sendStats(stats: ClientStats) {
    if (this.channel.readyState === "open") this.channel.send(encodeStats(stats));
  }

  /** Every animation frame: send the input state if it changed or the heartbeat is due. */
  private pump = () => {
    this.send();
    this.raf = requestAnimationFrame(this.pump);
  };

  /** In a hidden tab, where animation frames stop: the heartbeat goes on (`startTicker`). */
  private hiddenTick = () => {
    if (document.hidden) this.send();
  };

  private send() {
    if (this.channel.readyState === "open") {
      // A hidden tab holds nothing pressed (a key let go there is never heard); the menu's
      // pause holds, so a game paused under the menu stays paused.
      const reading = this.suspended || document.hidden ? null : this.input.read();
      const ports = reading?.ports ?? [0];
      const state = ports.join(",");
      // While suspended (the menu is open) the game is paused, not just left without input.
      const hotkeys = { ...(reading?.hotkeys ?? { fastForward: false, rewind: false }), pause: this.suspended };
      const bits = hotkeyBits(hotkeys);
      const now = performance.now();
      const heartbeat = now - this.lastSend > HEARTBEAT_MS;
      if (state !== this.sent || bits !== this.sentHotkeys || heartbeat) {
        this.seq = (this.seq + 1) >>> 0;
        this.channel.send(encodeInput({ seq: this.seq, clientTimeMs: now32(), ports }));
        // Held hotkeys are refreshed with every heartbeat; the worker drops stale ones.
        if (bits !== this.sentHotkeys || bits !== 0) this.channel.send(encodeHotkeys(hotkeys));
        this.sent = state;
        this.sentHotkeys = bits;
        this.lastSend = now;
      }
      // Sticks: on change, and as a heartbeat while off-centre (a lost packet must not leave a
      // stick held). Clients without sticks never send this message.
      const analog = reading?.analog ?? [CENTRED];
      const offCentre = analog.some((a) => !analogEqual(a, CENTRED));
      const changed =
        analog.length !== this.sentAnalog.length || analog.some((a, i) => !analogEqual(a, this.sentAnalog[i]));
      if (changed || (offCentre && now - this.lastAnalogSend > HEARTBEAT_MS)) {
        this.analogSeq = (this.analogSeq + 1) >>> 0;
        this.channel.send(encodeAnalog({ seq: this.analogSeq, ports: analog }));
        this.sentAnalog = analog;
        this.lastAnalogSend = now;
      }
    }
  }

  private setLink(link: Link) {
    if (!supersedes(this.link, link)) return;
    this.link = link;
    this.events.onLink?.(link);
  }

  private onMessage(data: ArrayBuffer) {
    const pipeline = decodePipeline(data);
    if (pipeline) {
      const notice = qualityNotice(this.pipeline, pipeline);
      this.pipeline = pipeline;
      if (notice) this.events.onQuality?.(notice);
      // The check's result comes again with the pipeline: a lost `0x83` is not missed.
      if (pipeline.link && this.link?.state !== "done") this.setLink(pipeline.link);
      return;
    }
    const link = decodeLink(data);
    if (link) {
      this.setLink(link);
      return;
    }
    const ack = decodeAck(data);
    if (!ack) return;
    // The ack echoes the low 32 bits of the send time; put back the high ones from now.
    const now = performance.now();
    const age = (now32() - ack.clientTimeMs) >>> 0;
    this.latency.acked(now - age, now);
    this.inputRtts.push((now32() - ack.clientTimeMs) >>> 0);
    if (this.inputRtts.length > 60) this.inputRtts.shift();
  }
}

const now32 = () => Math.floor(performance.now()) >>> 0;
