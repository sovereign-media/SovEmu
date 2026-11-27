import { describe, expect, it } from "vitest";
import { clientPath, measuredLabel, networkItems, networkLabel, pairLabel, rateLabel, transportLabel } from "./link";
import type { Link } from "./protocol";

const host = { candidateType: "host", protocol: "udp" };
const relay = (relayProtocol: string) => ({ candidateType: "relay", protocol: "udp", relayProtocol });
const done = (over: Partial<Link> = {}): Link => ({
  state: "done",
  elapsedMs: 640,
  budgetMs: 2000,
  path: "relay-udp",
  estimateKbps: 18200,
  startKbps: 12000,
  rttMs: 24.5,
  stoppedBy: "ceiling",
  ...over,
});

describe("the path this client reports", () => {
  it("says which of the relay's transports it uses, which only it can tell", () => {
    // The relay forwards UDP to the worker whatever the client used: without this the worker
    // could not start a TCP-relayed player lower, as it must.
    expect(clientPath(relay("tcp"))).toBe("relay-tcp");
    expect(clientPath(relay("tls"))).toBe("relay-tls");
    expect(clientPath(relay("udp"))).toBe("relay-udp");
    expect(clientPath({ candidateType: "relay" })).toBe("relay-udp");
    expect(clientPath(host)).toBe("direct");
    expect(clientPath({ candidateType: "srflx" })).toBe("direct");
    expect(clientPath(undefined)).toBeUndefined();
  });
});

describe("the Network rows", () => {
  it("names the pair, the transport and the network", () => {
    expect(pairLabel({ candidateType: "srflx" }, host)).toBe("STUN → host");
    expect(pairLabel(relay("udp"), host)).toBe("relay → host");
    expect(transportLabel(relay("tcp"))).toBe("TCP to the relay");
    expect(transportLabel(host)).toBe("UDP");
    // The loading screen says only where the player is; the details are in the overlay.
    expect(networkLabel("lan")).toBe("Home network");
    expect(networkLabel("relay-tcp")).toBe("Remote");
    expect(networkLabel("")).toBeNull();
    expect(rateLabel(18200)).toBe("18.2 Mbit/s");
    expect(rateLabel(850)).toBe("850 kbit/s");
  });

  it("says what was measured at the start, and when nothing was", () => {
    expect(measuredLabel(done())).toBe("18.2 Mbit/s in 0.6 s (reached the ceiling)");
    expect(measuredLabel(done({ stoppedBy: "off", estimateKbps: 0 }))).toBe("not measured (turned off)");
    expect(measuredLabel(null)).toBe("not measured");
    expect(measuredLabel(done({ state: "probing" }))).toBe("not measured");
  });

  it("shows this browser's own rows even with an older worker", () => {
    // TURN server, transport and path come from getStats: an older worker, or the check turned
    // off, must not hide them.
    const items = networkItems(relay("tcp"), host, null, 0);
    expect(items).toEqual([
      { label: "TURN server", value: "Yes" },
      { label: "Transport", value: "TCP to the relay" },
      { label: "Path", value: "relay → host" },
      { label: "Measured at start", value: "not measured" },
    ]);
    const full = networkItems(relay("udp"), host, done(), 15400);
    expect(full[0]).toEqual({ label: "Network", value: "Remote" });
    expect(full.slice(-2)).toEqual([
      { label: "Started at", value: "12.0 Mbit/s" },
      { label: "Estimate now", value: "15.4 Mbit/s" },
    ]);
  });
});
