import { describe, expect, it } from "vitest";
import type { NetworkReport } from "./api";
import { networkWarnings, portRange } from "./network";

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
  settings: [],
  ...over,
});

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
