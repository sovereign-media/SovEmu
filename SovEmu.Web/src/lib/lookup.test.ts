import { describe, expect, it } from "vitest";
import type { MetadataStatus } from "./api";
import { currentText, etaText, progressText, quotaPercent, quotaText, recentText } from "./lookup";

const ss = {
  id: "screenscraper",
  name: "ScreenScraper",
  asked: 3,
  found: 2,
  notFound: 1,
  errors: 0,
  quota: { requestsToday: 1500, maxRequestsPerDay: 20000, missesToday: 40, maxMissesPerDay: 2000 },
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
  artwork: { running: false, total: 0, done: 0, downloaded: 0, failed: 0 },
  current: { gameId: 1, title: "Castlevania", system: "nes", provider: "screenscraper", step: "ASKING" },
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

  it("names the game it is on and what it is doing for it", () => {
    expect(currentText(status({}))).toBe("Castlevania (nes): asking ScreenScraper");
    const current = { gameId: 1, title: "Castlevania", system: "nes", provider: null };
    expect(currentText(status({ current: { ...current, step: "HASHING" } }))).toBe(
      "Castlevania (nes): hashing the file",
    );
    expect(currentText(status({ current: { ...current, provider: "screenscraper", step: "WAITING" } }))).toBe(
      "Castlevania (nes): waiting for ScreenScraper",
    );
    expect(currentText(status({ current: null }))).toBeNull();
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
