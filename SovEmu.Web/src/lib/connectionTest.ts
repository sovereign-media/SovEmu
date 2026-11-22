// The admin's connection test: from this browser to a game port, as a game would connect.
//
// The server answers the offer with a peer on a free game port that has no media and echoes its
// data channel (`testConnection`). The channel is unordered with no retransmits, as a game's
// input is, so a ping that is lost stays lost. The round trip is timed on the pings; the path
// (straight to the port, through a STUN-discovered address, or through a TURN relay) and the
// address reached come from this browser's own WebRTC statistics.

import { median } from "./numbers";
import { iceGathered } from "./session";

/** How the browser reached the game port. */
export type IcePath = "direct" | "stun" | "relay";

export interface ConnectionReport {
  path: IcePath;
  /** `udp`, or `tcp` (to a TURN server over TCP or TLS). */
  protocol: string;
  /** The game port's address as the browser reached it. */
  remote: string | null;
  /** The round trip, timed on the pings (median and fastest), ms. */
  rttMs: number;
  rttMinMs: number;
  /** The round trip WebRTC itself measured on the connection, ms, where the browser says. */
  iceRttMs: number | null;
  sent: number;
  lost: number;
}

export type ConnectionResult =
  | { ok: true; report: ConnectionReport }
  /** `answer`: the server did not answer; `connect`: no route to the port; `echo`: none came back. */
  | { ok: false; stage: "answer" | "connect" | "echo"; reason: string };

/** A candidate's type as the path it means. */
export function pathOf(candidateType: string | undefined): IcePath {
  if (candidateType === "relay") return "relay";
  if (candidateType === "srflx" || candidateType === "prflx") return "stun";
  return "direct";
}

export const PATH_LABELS: Record<IcePath, string> = {
  direct: "Direct to the game port",
  stun: "Direct, through the address STUN found",
  relay: "Through a TURN relay",
};

type Stat = { id: string; type: string; [key: string]: unknown };

/**
 * The candidate pair in use, and its two ends: the transport's selected pair where the browser
 * names one, else the nominated pair that succeeded.
 */
export function selectedPair(stats: Iterable<Stat>): { pair: Stat; local?: Stat; remote?: Stat } | null {
  const byId = new Map<string, Stat>();
  for (const s of stats) byId.set(s.id, s);
  const all = [...byId.values()];
  const chosenId = all.find((s) => s.type === "transport" && s.selectedCandidatePairId)?.selectedCandidatePairId;
  const pair =
    (typeof chosenId === "string" ? byId.get(chosenId) : undefined) ??
    all.find((s) => s.type === "candidate-pair" && s.nominated && s.state === "succeeded");
  if (!pair) return null;
  return {
    pair,
    local: byId.get(pair.localCandidateId as string),
    remote: byId.get(pair.remoteCandidateId as string),
  };
}

/** The address and port the server's answer offers (its one host candidate), if it has one. */
export function answerAddress(sdp: string): string | null {
  for (const line of sdp.split(/\r?\n/)) {
    // a=candidate:<foundation> <component> <protocol> <priority> <address> <port> typ <type>
    const parts = line.startsWith("a=candidate:") ? line.split(" ") : [];
    if (parts.length >= 6) return parts[4].includes(":") ? `[${parts[4]}]:${parts[5]}` : `${parts[4]}:${parts[5]}`;
  }
  return null;
}

/** The report from what the browser's statistics say and the pings' round trips. */
export function report(stats: Iterable<Stat>, rtts: number[], sent: number): ConnectionReport {
  const chosen = selectedPair(stats);
  const remote = chosen?.remote;
  const address = remote?.address ?? remote?.ip;
  const iceRtt = chosen?.pair.currentRoundTripTime;
  return {
    path: pathOf(chosen?.local?.candidateType as string | undefined),
    protocol: String(chosen?.local?.relayProtocol ?? chosen?.local?.protocol ?? "udp"),
    remote: address ? `${address}:${remote?.port}` : null,
    rttMs: median(rtts),
    rttMinMs: rtts.length ? Math.min(...rtts) : NaN,
    iceRttMs: typeof iceRtt === "number" ? iceRtt * 1000 : null,
    sent,
    lost: sent - rtts.length,
  };
}

export interface TestOptions {
  iceServers: RTCIceServer[];
  /** Sends the offer (SDP) to the server and returns its answer (SDP). */
  answer: (sdp: string) => Promise<string>;
  pings?: number;
  connectMs?: number;
}

const PING_EVERY_MS = 100;
/** After the last ping, how long the echoes get to come back. */
const ECHO_WAIT_MS = 2000;

/** Runs the test once, and closes its connection however it ends. */
export async function runConnectionTest({
  iceServers,
  answer,
  pings = 10,
  connectMs = 10_000,
}: TestOptions): Promise<ConnectionResult> {
  const pc = new RTCPeerConnection({ iceServers });
  const channel = pc.createDataChannel("connection-test", { ordered: false, maxRetransmits: 0 });
  try {
    await pc.setLocalDescription(await pc.createOffer());
    await iceGathered(pc);
    let sdp: string;
    try {
      sdp = await answer(pc.localDescription?.sdp ?? "");
    } catch (e) {
      return { ok: false, stage: "answer", reason: (e as Error).message };
    }
    await pc.setRemoteDescription({ type: "answer", sdp });

    const opened = await new Promise<boolean>((resolve) => {
      if (channel.readyState === "open") return resolve(true);
      channel.onopen = () => resolve(true);
      pc.onconnectionstatechange = () => pc.connectionState === "failed" && resolve(false);
      setTimeout(() => resolve(false), connectMs);
    });
    if (!opened) {
      const where = answerAddress(sdp);
      return {
        ok: false,
        stage: "connect",
        reason: `This browser could not reach the game port${where ? ` at ${where}` : ""} over UDP${
          iceServers.length ? ", nor through the STUN or TURN servers" : ""
        }.`,
      };
    }

    const sentAt = new Map<number, number>();
    const rtts: number[] = [];
    channel.onmessage = (e) => {
      const at = sentAt.get(Number(e.data));
      if (at !== undefined) rtts.push(performance.now() - at);
    };
    for (let i = 0; i < pings; i++) {
      sentAt.set(i, performance.now());
      channel.send(String(i));
      await new Promise((r) => setTimeout(r, PING_EVERY_MS));
    }
    const deadline = performance.now() + ECHO_WAIT_MS;
    while (rtts.length < pings && performance.now() < deadline) await new Promise((r) => setTimeout(r, 50));
    if (!rtts.length) {
      return { ok: false, stage: "echo", reason: "Connected, but none of the pings came back." };
    }
    const stats = [...(await pc.getStats()).values()] as Stat[];
    return { ok: true, report: report(stats, rtts, pings) };
  } finally {
    pc.close();
  }
}
