import { describe, expect, it } from "vitest";
import { answerAddress, pathOf, report, selectedPair } from "./connectionTest";

const stats = (localType: string, extra: Record<string, unknown> = {}) => [
  { id: "T1", type: "transport", selectedCandidatePairId: "P2" },
  { id: "P1", type: "candidate-pair", localCandidateId: "L1", remoteCandidateId: "R1", state: "failed" },
  {
    id: "P2",
    type: "candidate-pair",
    localCandidateId: "L2",
    remoteCandidateId: "R1",
    state: "succeeded",
    nominated: true,
    currentRoundTripTime: 0.012,
  },
  { id: "L1", type: "local-candidate", candidateType: "host", protocol: "udp" },
  { id: "L2", type: "local-candidate", candidateType: localType, protocol: "udp", ...extra },
  { id: "R1", type: "remote-candidate", candidateType: "host", address: "192.168.1.20", port: 50003 },
];

describe("connection test", () => {
  it("names the path from the candidate in use", () => {
    expect(pathOf("host")).toBe("direct");
    expect(pathOf("srflx")).toBe("stun");
    expect(pathOf("prflx")).toBe("stun");
    expect(pathOf("relay")).toBe("relay");
    expect(pathOf(undefined)).toBe("direct");
  });

  it("finds the pair the transport selected, else the nominated one", () => {
    expect(selectedPair(stats("srflx"))?.local?.id).toBe("L2");
    const noTransport = stats("host").filter((s) => s.type !== "transport");
    expect(selectedPair(noTransport)?.pair.id).toBe("P2");
    expect(selectedPair([])).toBeNull();
  });

  it("reports the path, the address reached, the round trips and the losses", () => {
    expect(report(stats("relay", { relayProtocol: "tcp" }), [30, 20, 25], 4)).toEqual({
      path: "relay",
      protocol: "tcp",
      remote: "192.168.1.20:50003",
      rttMs: 25,
      rttMinMs: 20,
      iceRttMs: 12,
      sent: 4,
      lost: 1,
    });
  });

  it("reads the game port from the server's answer", () => {
    const sdp = "v=0\r\na=ice-lite\r\na=candidate:1 1 udp 2130706431 203.0.113.7 50001 typ host\r\n";
    expect(answerAddress(sdp)).toBe("203.0.113.7:50001");
    expect(answerAddress("a=candidate:1 1 udp 1 ::1 50002 typ host")).toBe("[::1]:50002");
    expect(answerAddress("v=0")).toBeNull();
  });
});
