import { describe, expect, it } from "vitest";
import { CLIENT_LABELS, lapsesText, signInName, usedText } from "./signIns";

describe("sign-ins", () => {
  const now = Date.parse("2026-11-22T12:00:00Z");
  const unix = now / 1000;

  it("names each client", () => {
    expect(CLIENT_LABELS.APP).toBe("App");
    expect(CLIENT_LABELS.WEB).toBe("Web browser");
  });

  it("says when a sign-in lapses", () => {
    expect(lapsesText(null, now)).toBe("Stays signed in until signed out");
    expect(lapsesText(unix + 3 * 86_400, now)).toBe("Lapses in 3 days unless used");
    expect(lapsesText(unix + 86_400, now)).toBe("Lapses in a day unless used");
    expect(lapsesText(unix + 3600, now)).toBe("Lapses today unless used");
    expect(lapsesText(unix - 2 * 86_400, now)).toBe("Lapsed");
  });

  it("says when it began and was last used", () => {
    expect(usedText({ signedInAt: unix - 4 * 86_400, lastUsedAt: unix - 10 }, now)).toBe(
      "Signed in 4 days ago · last used just now",
    );
  });

  it("names a sign-in by its device, else its kind of client", () => {
    expect(signInName({ client: "APP", device: "SovEmu for Android 1.4.0 · Mi TV" })).toBe(
      "SovEmu for Android 1.4.0 · Mi TV",
    );
    expect(signInName({ client: "APP", device: null })).toBe("App");
    expect(signInName({ client: "WEB", device: null }, { lower: true })).toBe("web browser");
    expect(signInName({ client: "APP", device: "Living-room TV" }, { lower: true })).toBe("Living-room TV");
  });
});
