import { describe, expect, it } from "vitest";
import type { StorageReport } from "./api";
import { relativePath, segments } from "./storage";

const report = (over: Partial<StorageReport>): StorageReport => ({
  measuredAt: 1_700_000_000,
  dataDir: "/data",
  totalBytes: 1000,
  freeBytes: 400,
  cacheLimitBytes: null,
  areas: [
    { id: "saves", path: "/data/saves", bytes: 300, files: 10 },
    { id: "database", path: "/data/sovemu.db", bytes: 100, files: 1 },
    { id: "mirror", path: "/data/library", bytes: 0, files: 0 },
  ],
  saves: [],
  ...over,
});

describe("the storage page", () => {
  it("splits the filesystem into SovEmu's areas, the rest and the free space", () => {
    expect(segments(report({})).map((s) => [s.id, s.bytes, s.percent])).toEqual([
      ["saves", 300, 30],
      ["database", 100, 10],
      ["elsewhere", 200, 20],
      ["free", 400, 40],
    ]);
    expect(segments(report({ totalBytes: null }))).toEqual([]);
  });
});

describe("paths", () => {
  it("are shown within the data folder", () => {
    expect(relativePath("/data/cache/games", "/data")).toBe("cache/games");
    expect(relativePath("/data", "/data/")).toBe(".");
    expect(relativePath("/opt/sovemu/decorations", "/data")).toBe("/opt/sovemu/decorations");
  });
});
