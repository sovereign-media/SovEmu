import { describe, expect, it } from "vitest";
import type { MetadataStatus } from "./api";
import {
  accountText,
  artworkText,
  etaText,
  failureText,
  progressText,
  quotaPercent,
  quotaText,
  recentText,
  workingText,
} from "./lookup";

const ss = {
  id: "screenscraper",
  name: "ScreenScraper",
  asked: 3,
  found: 2,
  notFound: 1,
  errors: 0,
  quota: { requestsToday: 1500, maxRequestsPerDay: 20000, missesToday: 40, maxMissesPerDay: 2000 },
  threads: 2,
  account: {
    user: "someone",
    level: 1,
    contribution: 2,
    maxThreads: 2,
    maxRequestsPerMinute: 120,
    maxRequestsPerDay: 20000,
    maxMissesPerDay: 2000,
    maxDownloadKbps: 128,
  },
};

const status = (over: Partial<MetadataStatus>): MetadataStatus => ({
  running: true,
  total: 1200,
  done: 300,
  found: 280,
  notFound: 15,
  failed: 5,
  sources: ["fbneo", "screenscraper"],
  notes: [],
  artwork: { running: false, total: 0, done: 0, downloaded: 0, failed: 0, kinds: [], failures: [] },
  working: [{ gameId: 1, title: "Castlevania", system: "nes", provider: "screenscraper", step: "ASKING" }],
  startedAt: 1_700_000_000,
  finishedAt: null,
  etaSeconds: 720,
  systems: [],
  providers: [ss],
  recent: [],
  ...over,
});

describe("a game-information lookup", () => {
  it("says how far it is and how long it has left", () => {
    expect(progressText(status({}))).toBe("Looking up 300 of 1,200 games · about 12 minutes left…");
    expect(progressText(status({ etaSeconds: null }))).toBe("Looking up 300 of 1,200 games…");
    expect(progressText(status({ running: false }))).toBe("Last run: 280 found, 15 not found, 5 to retry.");
    expect(etaText(20)).toBe("less than a minute left");
    expect(etaText(60)).toBe("about 1 minute left");
    expect(etaText(4 * 3600)).toBe("about 4 hours left");
  });

  it("names the games it is on and what it is doing for each", () => {
    const game = { gameId: 2, title: "Contra", system: "nes", provider: null };
    expect(
      workingText(
        status({
          working: [
            { gameId: 1, title: "Castlevania", system: "nes", provider: "screenscraper", step: "ASKING" },
            { ...game, step: "HASHING" },
            { ...game, gameId: 3, provider: "screenscraper", step: "WAITING" },
          ],
        }),
      ),
    ).toEqual([
      "Castlevania (nes): asking ScreenScraper",
      "Contra (nes): hashing the file",
      "Contra (nes): waiting for ScreenScraper",
    ]);
    expect(workingText(status({ working: [] }))).toEqual([]);
  });

  it("describes a source's account and the pool SovEmu uses", () => {
    expect(accountText(ss)).toBe(
      "someone · level 1 · contribution 2 · 2 requests at once · 120 a minute · 20,000 a day · downloads up to 128 KB/s",
    );
    expect(accountText({ ...ss, threads: 1 })).toContain("1 of 2 requests at once");
    const guest = { ...ss, threads: 1, account: { ...ss.account, user: null, contribution: 0, maxThreads: null } };
    expect(accountText(guest)).toMatch(/^level 1 · 1 request at once/);
    expect(accountText({ ...ss, account: null })).toBe("");
  });

  it("shows a source's quota, its misses included", () => {
    expect(quotaText(ss)).toBe("1,500 of 20,000 requests today · 40 of 2,000 not found");
    expect(quotaPercent(ss)).toBe(7.5);
    const unknown = {
      ...ss,
      quota: { requestsToday: 3, maxRequestsPerDay: null, missesToday: 0, maxMissesPerDay: null },
    };
    expect(quotaText(unknown)).toBe("3 requests today");
    expect(quotaPercent(unknown)).toBeNull();
    expect(quotaText({ ...ss, quota: null })).toBe("");
  });

  it("says what happened to each game just looked up", () => {
    const s = status({});
    const r = { gameId: 1, title: "Castlevania", system: "nes", sources: ["screenscraper"], detail: null };
    expect(recentText(s, { ...r, outcome: "FOUND" })).toBe("found by ScreenScraper");
    expect(recentText(s, { ...r, outcome: "NOT_FOUND", sources: [] })).toBe("not found");
    expect(recentText(s, { ...r, outcome: "FAILED", sources: [], detail: "quota exhausted" })).toBe(
      "to retry: quota exhausted",
    );
  });
});

describe("artwork downloads", () => {
  const kinds = [
    { kind: "BOX_ART" as const, total: 400, downloaded: 400, failed: 0 },
    { kind: "SCREENSHOT" as const, total: 0, downloaded: 0, failed: 0 },
    { kind: "MARQUEE" as const, total: 0, downloaded: 0, failed: 0 },
    { kind: "VIDEO" as const, total: 412, downloaded: 0, failed: 412 },
  ];
  const art = { running: false, total: 812, done: 812, downloaded: 400, failed: 412, kinds, failures: [] };

  it("says each kind it queued, and what failed", () => {
    expect(artworkText(art)).toEqual({
      summary: "Artwork and videos: 400 downloaded, 412 failed.",
      kinds: ["Box art: 400 of 400 downloaded", "Videos: 0 of 412 downloaded, 412 failed"],
    });
    expect(artworkText({ ...art, running: true, done: 10 }).summary).toBe(
      "Downloading artwork and videos in the background: 10 of 812…",
    );
  });

  it("says why downloads failed, and how often", () => {
    expect(failureText({ kind: "VIDEO", reason: "not an MP4 video: API closed", count: 412 })).toBe(
      "Video: not an MP4 video: API closed (×412)",
    );
    expect(failureText({ kind: "BOX_ART", reason: "no answer (timed out)", count: 1 })).toBe(
      "Box art: no answer (timed out)",
    );
  });
});
