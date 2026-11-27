// The admin Network page's checks: what is likely to stop a browser signing in or a game
// streaming, said plainly. No React, so Vitest can check them.

import type { NetworkReport } from "./api";

export interface NetworkWarning {
  title: string;
  detail: string;
}

/** What may go wrong for clients, given how the server is set up and how this page reached it. */
export function networkWarnings(n: NetworkReport): NetworkWarning[] {
  const warnings: NetworkWarning[] = [];
  if (!n.signInAvailableHere) {
    warnings.push({
      title: `Signing in does not work at ${n.requestHost || "this address"}`,
      detail: `Sign-in works only at the server's sign-in addresses (${n.publicUrls.join(", ")}). Add this address to SovEmu__Http__PublicUrl, or open the server at one of them. The check compares the host and port only: behind a proxy that serves HTTPS, list the https:// address.`,
    });
  }
  if (n.advertisedIpDetected) {
    warnings.push({
      title: `Game streams go to ${n.advertisedIp}, an address the server worked out`,
      detail:
        "That is the machine's own primary address. In a container it is the container's, which browsers cannot reach: set SovEmu__Streaming__AdvertiseAddress to the host's address.",
    });
  }
  if (n.portCount > 0 && n.portsInUse >= n.portCount) {
    warnings.push({
      title: "Every game port is in use",
      detail: `All ${n.portCount} UDP ports are held, so no one else can start a game. Widen SovEmu__Streaming__Udp__Ports (and publish the range).`,
    });
  }
  const pm = n.portMapping;
  if (pm.state === "ACTIVE" && pm.externalAddressPrivate) {
    warnings.push({
      title: `The router's own internet address, ${pm.externalAddress ?? ""}, is a private one`,
      detail:
        "Another router, or the internet provider's carrier-grade NAT, is in front of it, so the ports it forwards can't be reached from the internet. Players away from home connect through STUN or the built-in relay, if anything.",
    });
  }
  return warnings;
}

type PortMapping = NetworkReport["portMapping"];

/** One line on asking the router to forward ports: whether it was asked, and how it went. */
export function portMappingSummary(pm: PortMapping): string {
  switch (pm.state) {
    case "OFF":
      return "Not asked: SovEmu__Network__PortMapping is off. Forward the ports by hand if players away from home need them.";
    case "SEARCHING":
      return "Looking for the router (UPnP and NAT-PMP).";
    case "NO_ROUTER":
      return "No router answered UPnP or NAT-PMP: it has them turned off, or the server is in a container on a bridge network, where the search can't reach it. Searched again every ten minutes.";
    case "STOPPED":
      return "Stopped: the server is shutting down, and the router's forwards were removed.";
    case "ACTIVE": {
      const forwarded = pm.ports.filter((p) => p.external != null).length;
      return `The router (${pm.protocol ?? "unknown"}, at ${pm.router ?? "?"}) forwards ${forwarded} of ${pm.ports.length} ports to ${pm.localAddress ?? "this server"}.`;
    }
    default:
      // A state from a newer server.
      return `${String(pm.state).toLowerCase()}.`;
  }
}

/** "50000/udp → 51000", or why the router doesn't forward it. */
export function forwardedPort(p: PortMapping["ports"][number]): string {
  const name = `${p.internal}/${p.transport}`;
  if (p.external == null) return `${name}: ${p.error ?? "not forwarded yet"}`;
  return p.external === p.internal ? `${name}: forwarded` : `${name}: forwarded from ${p.external}`;
}

/** "50000-50009" or "50000" for a single port. */
export function portRange(n: Pick<NetworkReport, "udpFirst" | "udpLast">): string {
  return n.udpFirst === n.udpLast ? `${n.udpFirst}` : `${n.udpFirst}-${n.udpLast}`;
}
