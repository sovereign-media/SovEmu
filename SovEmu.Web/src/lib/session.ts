// One WebRTC connection to a game session: receive-only video and audio, plus the unordered,
// no-retransmit input channel. Signalling is one exchange of the complete offer (non-trickle) for
// the worker's answer: the `sessionOffer` mutation, passed in as `signal`.
import { InputState } from "./input";
import { type Analog, CENTRED, INPUT_CHANNEL, analogEqual, decodeAck, encodeAnalog, encodeHotkeys, encodeInput, hotkeyBits } from "./protocol";

/** Send at least this often even when nothing changed: the channel drops lost packets. */
const HEARTBEAT_MS = 100;
const ICE_GATHER_TIMEOUT_MS = 1500;

export type SessionState = RTCPeerConnectionState | "idle";

export interface SessionEvents {
  onState?: (state: SessionState) => void;
}

export interface SessionOptions {
  /** STUN/TURN servers from the server's configuration (`clientConfig.iceServers`). */
  iceServers?: RTCIceServer[];
}

export class Session {
  readonly pc: RTCPeerConnection;
  /** Recent input round trips (send -> server applies on a frame -> ack), in ms. */
  readonly inputRtts: number[] = [];
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
    this.pc = new RTCPeerConnection({ iceServers: options.iceServers ?? [] });
    this.pc.addTransceiver("video", { direction: "recvonly" });
    this.pc.addTransceiver("audio", { direction: "recvonly" });
    this.channel = this.pc.createDataChannel(INPUT_CHANNEL, { ordered: false, maxRetransmits: 0 });
    this.channel.binaryType = "arraybuffer";
    this.channel.onmessage = (e) => this.onAck(e.data as ArrayBuffer);

    const stream = new MediaStream();
    video.srcObject = stream;
    this.pc.ontrack = (e) => {
      // Ask for the smallest jitter buffer the browser allows: latency over smoothness.
      const receiver = e.receiver as RTCRtpReceiver & { jitterBufferTarget?: number; playoutDelayHint?: number };
      try { receiver.jitterBufferTarget = 0; } catch { /* unsupported */ }
      try { receiver.playoutDelayHint = 0; } catch { /* unsupported */ }
      stream.addTrack(e.track);
    };
    this.pc.onconnectionstatechange = () => this.events.onState?.(this.pc.connectionState);
  }

  async connect(): Promise<void> {
    await this.pc.setLocalDescription(await this.pc.createOffer());
    await this.iceGathered();
    const answer = await this.signal(this.pc.localDescription?.sdp ?? "");
    await this.pc.setRemoteDescription({ type: "answer", sdp: answer });
    this.video.play().catch(() => { /* needs a user gesture; Connect is one */ });
    this.raf = requestAnimationFrame(this.pump);
  }

  close(): void {
    cancelAnimationFrame(this.raf);
    this.pc.close();
  }

  private iceGathered(): Promise<void> {
    if (this.pc.iceGatheringState === "complete") return Promise.resolve();
    return new Promise((resolve) => {
      this.pc.addEventListener("icegatheringstatechange", () => {
        if (this.pc.iceGatheringState === "complete") resolve();
      });
      setTimeout(resolve, ICE_GATHER_TIMEOUT_MS);
    });
  }

  /** Every animation frame: send the input state if it changed or the heartbeat is due. */
  private pump = () => {
    if (this.channel.readyState === "open") {
      const reading = this.suspended ? null : this.input.read();
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
    this.raf = requestAnimationFrame(this.pump);
  };

  private onAck(data: ArrayBuffer) {
    const ack = decodeAck(data);
    if (!ack) return;
    this.inputRtts.push((now32() - ack.clientTimeMs) >>> 0);
    if (this.inputRtts.length > 60) this.inputRtts.shift();
  }
}

const now32 = () => Math.floor(performance.now()) >>> 0;
