import { describe, expect, it } from "vitest";
import { isWarning, KIND_LABELS, toCsv } from "./activity";
import type { ActivityEntry } from "./api";

const entry = (over: Partial<ActivityEntry>): ActivityEntry => ({
  at: Date.parse("2026-11-22T12:00:00Z") / 1000,
  userId: 1,
  username: "admin",
  kind: "SIGNED_IN",
  category: "SIGN_IN",
  subject: null,
  detail: null,
  address: "192.168.1.20",
  ...over,
});

describe("activity", () => {
  it("names each kind, and tells refusals apart", () => {
    expect(KIND_LABELS.ROLE_CHANGED).toBe("Changed a role");
    expect(isWarning("SIGN_IN_FAILED")).toBe(true);
    expect(isWarning("GAME_ENDED")).toBe(false);
  });

  it("exports CSV with a header, ISO times, and quoted fields where needed", () => {
    const csv = toCsv([
      entry({}),
      entry({ kind: "GAME_STARTED", subject: 'Zelda, "Link\'s" Awakening', detail: "gb", address: null }),
    ]);
    expect(csv.split("\r\n")).toEqual([
      "time,user,what,subject,detail,address",
      "2026-11-22T12:00:00.000Z,admin,Signed in,,,192.168.1.20",
      '2026-11-22T12:00:00.000Z,admin,Started a game,"Zelda, ""Link\'s"" Awakening",gb,',
      "",
    ]);
  });
});
