import { describe, expect, it } from "vitest";
import type { NetworkReport } from "./api";
import { forwardedPort, networkWarnings, portMappingSummary, portRange } from "./network";

const report = (over: Partial<NetworkReport>): NetworkReport => ({
  http: "0.0.0.0:8080",
  udpFirst: 50000,
  udpLast: 50009,
  portCount: 10,
  portsInUse: 1,
  sessions: 1,
  udpBind: "0.0.0.0",
  advertisedIp: "192.168.1.10",
  advertisedIpDetected: false,
  publicUrls: ["http://192.168.1.10:8080"],
  publicUrlsConfigured: true,
  signInAvailableHere: true,
  requestHost: "192.168.1.10:8080",
  clientIp: "192.168.1.20",
  forwardedFor: null,
  trustedProxies: [],
  secureCookies: false,
  iceServers: [],
  portMapping: mapping({}),
  settings: [],
  ...over,
});

type PortMapping = NetworkReport["portMapping"];

function mapping(over: Partial<PortMapping>): PortMapping {
  return {
    state: "ACTIVE",
    protocol: "UPnP",
    router: "192.168.1.1",
    localAddress: "192.168.1.10",
    externalAddress: "203.0.113.7",
    externalAddressPrivate: false,
    ports: [
      { transport: "udp", internal: 50000, external: 50000, error: null },
      { transport: "udp", internal: 50001, external: 51001, error: null },
      { transport: "tcp", internal: 3478, external: null, error: "another device on the network has this port" },
    ],
    ...over,
  };
}

describe("the network page", () => {
  it("is quiet when all is well", () => {
    expect(networkWarnings(report({}))).toEqual([]);
  });

  it("warns about each thing that stops clients", () => {
    const titles = networkWarnings(
      report({ signInAvailableHere: false, requestHost: "games.example", advertisedIpDetected: true, portsInUse: 10 }),
    ).map((w) => w.title);
    expect(titles).toEqual([
      "Signing in does not work at games.example",
      "Game streams go to 192.168.1.10, an address the server worked out",
      "Every game port is in use",
    ]);
  });

  it("writes port ranges", () => {
    expect(portRange({ udpFirst: 50000, udpLast: 50009 })).toBe("50000-50009");
    expect(portRange({ udpFirst: 6000, udpLast: 6000 })).toBe("6000");
  });
});

describe("asking the router to forward ports", () => {
  it("warns when the router's own address can't be reached from the internet", () => {
    // Behind double or carrier-grade NAT the forwards exist but lead nowhere: say so, since the
    // page would otherwise read as a success.
    const pm = mapping({ externalAddress: "100.64.1.2", externalAddressPrivate: true });
    expect(networkWarnings(report({ portMapping: pm })).map((w) => w.title)).toEqual([
      "The router's own internet address, 100.64.1.2, is a private one",
    ]);
    expect(networkWarnings(report({ portMapping: { ...pm, state: "NO_ROUTER" } }))).toEqual([]);
  });

  it("counts only the ports the router forwards", () => {
    expect(portMappingSummary(mapping({}))).toBe(
      "The router (UPnP, at 192.168.1.1) forwards 2 of 3 ports to 192.168.1.10.",
    );
  });

  it("says why nothing was forwarded", () => {
    expect(portMappingSummary(mapping({ state: "OFF" }))).toContain("SovEmu__Network__PortMapping is off");
    expect(portMappingSummary(mapping({ state: "NO_ROUTER" }))).toContain("No router answered");
  });

  it("shows another external port, and a refusal's reason", () => {
    const [same, other, refused] = mapping({}).ports;
    expect([same, other, refused].map(forwardedPort)).toEqual([
      "50000/udp: forwarded",
      "50001/udp: forwarded from 51001",
      "3478/tcp: another device on the network has this port",
    ]);
  });
});
