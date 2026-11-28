// Where this client's link goes and how it was measured (#243): the path it reports to the
// worker, the stats overlay's Network rows, and the loading screen's "Home network" / "Remote".
// The Android and iOS apps word these the same way.

import type { Link } from "./protocol";
import type { StatItem } from "./stats";

type Stat = { [key: string]: unknown };

/**
 * The path this client reports in its stats (`ClientStats.path`): through the relay over
 * `relay-udp`, `relay-tcp` or `relay-tls` when its own end of the pair is a relay candidate,
 * else `direct`. Only the client can tell the relay's transports apart.
 */
export function clientPath(local: Stat | undefined): string | undefined {
  if (!local) return undefined;
  if (local.candidateType !== "relay") return "direct";
  const protocol = String(local.relayProtocol ?? "udp").toLowerCase();
  return `relay-${protocol === "tcp" || protocol === "tls" ? protocol : "udp"}`;
}

/**
 * Whether a check report should replace the one shown. The input channel is unordered, so a
 * "probing" report can arrive after "done" (or after a later "probing"): it is stale, and would
 * send the loading bar's label and the Network rows back. A client checks its link once.
 */
export function supersedes(current: Link | null, next: Link): boolean {
  if (!current) return true;
  if (current.state === "done") return next.state === "done";
  return next.state === "done" || next.elapsedMs >= current.elapsedMs;
}

const CANDIDATES: Record<string, string> = { host: "host", srflx: "STUN", prflx: "STUN", relay: "relay" };

/** `host → host`, `STUN → host`, `relay → host`: the selected pair's candidate types. */
export function pairLabel(local: Stat | undefined, remote: Stat | undefined): string | null {
  const name = (s: Stat | undefined) => CANDIDATES[String(s?.candidateType)] ?? null;
  const [l, r] = [name(local), name(remote)];
  return l && r ? `${l} → ${r}` : null;
}

/** `UDP`, or `TCP to the relay` / `TLS to the relay` when relayed. */
export function transportLabel(local: Stat | undefined): string | null {
  if (!local) return null;
  if (local.candidateType === "relay") {
    return `${String(local.relayProtocol ?? "udp").toUpperCase()} to the relay`;
  }
  return String(local.protocol ?? "udp").toUpperCase();
}

/** The loading screen's word for where the player is: `Home network` on `lan`, else `Remote`. */
export function networkLabel(path: string): string | null {
  if (!path) return null;
  return path === "lan" ? "Home network" : "Remote";
}

/** `18.2 Mbit/s`, or `850 kbit/s` under a megabit. */
export function rateLabel(kbps: number): string {
  return kbps >= 1000 ? `${(kbps / 1000).toFixed(1)} Mbit/s` : `${Math.round(kbps)} kbit/s`;
}

const STOPPED: Record<string, string> = {
  ceiling: "reached the ceiling",
  steady: "settled",
  trouble: "the link filled",
  budget: "out of time",
  unavailable: "could not measure",
  off: "turned off",
};

/** `18.2 Mbit/s in 0.6 s (reached the ceiling)`, `not measured`. */
export function measuredLabel(link: Link | null): string {
  if (
    !link ||
    link.state !== "done" ||
    link.stoppedBy === "off" ||
    link.stoppedBy === "unavailable" ||
    !link.estimateKbps
  ) {
    return link?.stoppedBy === "off" ? "not measured (turned off)" : "not measured";
  }
  const why = STOPPED[link.stoppedBy] ?? link.stoppedBy;
  return `${rateLabel(link.estimateKbps)} in ${(link.elapsedMs / 1000).toFixed(1)} s (${why})`;
}

/**
 * The stats overlay's Network rows: where the link goes and through what (this browser's own
 * view, shown with any worker), then what the worker measured before the stream started and
 * estimates now (blank with an older worker).
 */
export function networkItems(
  local: Stat | undefined,
  remote: Stat | undefined,
  link: Link | null,
  estimateKbps: number,
): StatItem[] {
  const items: StatItem[] = [];
  const network = networkLabel(link?.path ?? "");
  if (network) items.push({ label: "Network", value: network });
  if (local) items.push({ label: "TURN server", value: local.candidateType === "relay" ? "Yes" : "No" });
  const transport = transportLabel(local);
  if (transport) items.push({ label: "Transport", value: transport });
  const pair = pairLabel(local, remote);
  if (pair) items.push({ label: "Path", value: pair });
  items.push({ label: "Measured at start", value: measuredLabel(link) });
  if (link?.state === "done" && link.startKbps) items.push({ label: "Started at", value: rateLabel(link.startKbps) });
  if (estimateKbps) items.push({ label: "Estimate now", value: rateLabel(estimateKbps) });
  return items;
}
