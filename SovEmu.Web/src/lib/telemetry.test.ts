import { describe, expect, it } from "vitest";
import { exportText, intervalText } from "./telemetry";

const now = 1_700_000_000_000;

describe("the telemetry page", () => {
  it("says how each signal's exports went", () => {
    const base = { lastAt: null, ok: false, error: null, exports: 0, failures: 0 };
    expect(exportText(base, now)).toEqual({ tone: "quiet", text: "No export yet" });
    expect(exportText({ ...base, lastAt: now / 1000 - 60, ok: true, exports: 12 }, now)).toEqual({
      tone: "good",
      text: "Sent 1 min ago · 12 exports, 0 failed",
    });
    expect(exportText({ ...base, lastAt: now / 1000, exports: 1, failures: 1 }, now)).toEqual({
      tone: "bad",
      text: "Failed just now · 1 export, 1 failed",
    });
  });

  it("writes the metric interval", () => {
    expect(intervalText(60_000)).toBe("every 60 s");
    expect(intervalText(1500)).toBe("every 1.5 s");
  });
});
