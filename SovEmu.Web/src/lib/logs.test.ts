import { describe, expect, it } from "vitest";
import { fieldsText } from "./logs";

describe("logs", () => {
  it("writes fields as key=value, quoting values with spaces", () => {
    expect(
      fieldsText([
        { key: "job", value: "scan" },
        { key: "error", value: "no such file" },
      ]),
    ).toBe('job=scan error="no such file"');
    expect(fieldsText([])).toBe("");
  });
});
