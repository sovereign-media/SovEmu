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
      detail: `Sign-in works only at the server's sign-in addresses (${n.publicUrls.join(", ")}). Add this address to SOVEMU_PUBLIC_URL, or open the server at one of them. The check compares the host and port only: behind a proxy that serves HTTPS, list the https:// address.`,
    });
  }
  if (n.advertisedIpDetected) {
    warnings.push({
      title: `Game streams go to ${n.advertisedIp}, an address the server worked out`,
      detail:
        "That is the machine's own primary address. In a container it is the container's, which browsers cannot reach: set SOVEMU_ADVERTISE_IP to the host's address.",
    });
  }
  if (n.portCount > 0 && n.portsInUse >= n.portCount) {
    warnings.push({
      title: "Every game port is in use",
      detail: `All ${n.portCount} UDP ports are held, so no one else can start a game. Widen SOVEMU_UDP_PORTS (and publish the range).`,
    });
  }
  return warnings;
}

/** "50000-50009" or "50000" for a single port. */
export function portRange(n: Pick<NetworkReport, "udpFirst" | "udpLast">): string {
  return n.udpFirst === n.udpLast ? `${n.udpFirst}` : `${n.udpFirst}-${n.udpLast}`;
}
