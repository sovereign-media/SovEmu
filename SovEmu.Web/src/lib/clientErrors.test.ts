import { describe, expect, it } from "vitest";
import { isFault, KIND_LABELS, splitClient } from "./clientErrors";

describe("client errors", () => {
  it("names every kind, and tells faults from notes", () => {
    expect(KIND_LABELS.STREAM_STALLED).toBe("Picture stalled");
    expect(isFault("CONNECTION_FAILED")).toBe(true);
    expect(isFault("CONTROLLER_UNKNOWN")).toBe(false);
    expect(isFault("OTHER")).toBe(false);
  });

  it("splits a client's name into its app and device", () => {
    expect(splitClient("android 1.4.0 (Mi TV, Android 11)")).toEqual({
      app: "android 1.4.0",
      device: "Mi TV, Android 11",
    });
    expect(splitClient("web")).toEqual({ app: "web", device: null });
    expect(splitClient("ios 2.0 (iPad (10th generation))")).toEqual({
      app: "ios 2.0",
      device: "iPad (10th generation)",
    });
  });
});
