import { describe, expect, it } from "vitest";
import { ApiError } from "./graphql";
import { failureMessage, formatCode, isCompleteCode, normalizeCode } from "./signInTv";

describe("normalizeCode", () => {
  it("upper-cases what a phone keyboard lower-cases, and drops the dash and spaces", () => {
    // A phone's keyboard types lower case and a paste brings spaces: the field must still hold
    // the code the TV shows, or the person thinks they typed it wrong.
    expect(normalizeCode("qg3w-ry4w")).toBe("QG3WRY4W");
    expect(normalizeCode(" QG3W RY4W ")).toBe("QG3WRY4W");
  });

  it("keeps no more than the code's eight characters", () => {
    expect(normalizeCode("QG3WRY4WXX")).toBe("QG3WRY4W");
  });
});

describe("formatCode", () => {
  it("draws the dash after the fourth character, as the TV shows it", () => {
    expect(formatCode("qg3wry4w")).toBe("QG3W-RY4W");
    expect(formatCode("QG3WR")).toBe("QG3W-R");
  });

  it("adds no dash until there is something after it", () => {
    // A dash typed after four characters would otherwise be stuck: deleting it puts it back.
    expect(formatCode("QG3W")).toBe("QG3W");
    expect(formatCode("QG3W-")).toBe("QG3W");
  });
});

describe("isCompleteCode", () => {
  it("is true only with all eight characters, dash or not", () => {
    expect(isCompleteCode("QG3W-RY4")).toBe(false);
    expect(isCompleteCode("QG3W-RY4W")).toBe(true);
    expect(isCompleteCode("qg3wry4w")).toBe(true);
  });
});

describe("failureMessage", () => {
  it("says a code no TV is waiting with is the code or the TV's server, not a fault", () => {
    const message = failureMessage(new ApiError("NOT_FOUND", "no device"));
    expect(message).toMatch(/^No TV is waiting with that code\./);
  });

  it("asks to wait after too many codes", () => {
    expect(failureMessage(new ApiError("TOO_MANY_REQUESTS", "x"))).toMatch(/Try again in a minute/);
  });

  it("shows the server's own sentence for anything else", () => {
    expect(failureMessage(new ApiError("INTERNAL", "The database is busy."))).toBe("The database is busy.");
    expect(failureMessage("?")).toBe("The TV could not be signed in.");
  });
});
