// Latency and stream health from RTCPeerConnection.getStats(), sampled once a second.
import type { Session } from "./session";

export interface StatItem {
  label: string;
  value: string;
}

type Sample = { t: number; video?: RTCInboundRtpStreamStats; audio?: RTCInboundRtpStreamStats };

const fmt = (x: number, unit = "", digits = 0) => (Number.isFinite(x) ? `${x.toFixed(digits)}${unit}` : "–");

function median(values: number[]): number {
  if (!values.length) return NaN;
  const s = [...values].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
}

export class StatsSampler {
  private last: Sample | null = null;

  async sample(session: Session): Promise<StatItem[]> {
    const report = await session.pc.getStats();
    const now: Sample = { t: performance.now() };
    let pair: RTCIceCandidatePairStats | undefined;
    report.forEach((s) => {
      if (s.type === "inbound-rtp" && s.kind === "video") now.video = s;
      if (s.type === "inbound-rtp" && s.kind === "audio") now.audio = s;
      if (s.type === "candidate-pair" && s.nominated && s.state === "succeeded") pair = s;
    });
    const prev = this.last;
    this.last = now;
    const v = now.video as (RTCInboundRtpStreamStats & Record<string, number>) | undefined;
    const pv = prev?.video as (RTCInboundRtpStreamStats & Record<string, number>) | undefined;
    if (!v) return [];

    const delta = (key: string) => (pv ? (v[key] ?? 0) - (pv[key] ?? 0) : 0);
    const dt = prev ? (now.t - prev.t) / 1000 : 0;
    const kbps = dt ? (delta("bytesReceived") * 8) / 1000 / dt : NaN;
    const emitted = delta("jitterBufferEmittedCount");
    const jitterMs = emitted ? (delta("jitterBufferDelay") / emitted) * 1000 : NaN;
    const decoded = delta("framesDecoded");
    const decodeMs = decoded ? (delta("totalDecodeTime") / decoded) * 1000 : NaN;
    const netRtt = pair?.currentRoundTripTime !== undefined ? pair.currentRoundTripTime * 1000 : NaN;
    const inputRtt = median(session.inputRtts);
    // Button to pixel, roughly: the input round trip (input reaches the core and the frame
    // comes back), plus half a frame of alignment, ~2 ms encode, jitter buffer, decode and
    // half a display refresh at 60 Hz.
    const estimate = inputRtt + 8 + 2 + (jitterMs || 0) + (decodeMs || 0) + 8;
    const lost = (v.packetsLost ?? 0) + (now.audio?.packetsLost ?? 0);

    return [
      { label: "Estimated button→pixel", value: fmt(estimate, " ms") },
      { label: "Input round trip", value: fmt(inputRtt, " ms") },
      { label: "Network RTT", value: fmt(netRtt, " ms", 1) },
      { label: "Jitter buffer", value: fmt(jitterMs, " ms", 1) },
      { label: "Decode", value: fmt(decodeMs, " ms", 1) },
      { label: "Video", value: `${v.frameWidth ?? "?"}×${v.frameHeight ?? "?"} @ ${fmt(v.framesPerSecond ?? NaN)} fps` },
      { label: "Bitrate", value: fmt(kbps, " kbps") },
      { label: "Packets lost", value: String(lost) },
    ];
  }
}
