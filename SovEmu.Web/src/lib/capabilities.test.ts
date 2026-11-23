import { describe, expect, it } from "vitest";
import { type AskDecoder, CAPABILITIES_VERSION, document, refreshRate, videoDecoders } from "./capabilities";

const screen = { width: 1512, height: 982, pixelRatio: 2, refreshHz: 120 };

describe("the capabilities a browser sends", () => {
  it("has a row for each codec the browser decodes, hardware where it says power efficient", async () => {
    // A laptop: VP8 and VP9 on the CPU, H.264 in hardware, no AV1, and H.265 cannot be asked.
    const ask: AskDecoder = async (contentType) => {
      if (contentType.startsWith("video/H265")) throw new TypeError("unknown content type");
      if (contentType.startsWith("video/AV1")) return { supported: false, powerEfficient: false };
      return { supported: true, powerEfficient: contentType.startsWith("video/H264") };
    };
    expect(await videoDecoders(ask)).toEqual([
      { codec: "VP8", hardware: false, lowLatency: true, name: null },
      { codec: "H264", hardware: true, lowLatency: true, name: null },
      { codec: "VP9", hardware: false, lowLatency: true, name: null },
    ]);
  });

  it("asks about H.264 as the server sends it: Constrained Baseline, packetization mode 1", async () => {
    const asked: string[] = [];
    await videoDecoders(async (contentType) => {
      asked.push(contentType);
      return { supported: true, powerEfficient: false };
    });
    expect(asked).toContain("video/H264;level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f");
    expect(asked).toHaveLength(5);
  });

  it("is a whole document of this version, with the screen in device pixels", () => {
    const rows = [{ codec: "VP8" as const, hardware: false, lowLatency: true, name: null }];
    expect(document(rows, screen)).toEqual({
      version: CAPABILITIES_VERSION,
      videoDecoders: rows,
      display: { width: 3024, height: 1964, refreshHz: 120 },
    });
  });

  it("says VP8 on the CPU for a browser that cannot be asked, so the document is never empty", () => {
    expect(document([], screen).videoDecoders).toEqual([
      { codec: "VP8", hardware: false, lowLatency: true, name: null },
    ]);
  });

  it("reads the refresh rate from the gaps between frames, and says 60 without enough of them", () => {
    expect(refreshRate([16.6, 16.7, 16.7, 16.6, 16.8, 33.4, 16.7])).toBe(60);
    expect(refreshRate([8.3, 8.4, 8.3, 8.3, 8.4, 8.3])).toBe(120);
    expect(refreshRate([6.9, 7.0, 6.9, 6.9, 7.0, 6.9])).toBe(144);
    expect(refreshRate([12.5, 12.4, 12.6, 12.5, 12.5, 12.5])).toBe(80);
    expect(refreshRate([16.7, 16.7])).toBe(60);
    expect(refreshRate([])).toBe(60);
  });
});
