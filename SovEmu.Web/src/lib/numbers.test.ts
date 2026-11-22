import { describe, expect, it } from "vitest";
import { formatBytes, median } from "./numbers";

describe("numbers", () => {
  it("takes the median, the upper one of an even count", () => {
    expect(median([])).toBeNaN();
    expect(median([5, 1, 3])).toBe(3);
    expect(median([4, 1, 3, 2])).toBe(3);
  });

  it("says a size in bytes, one decimal under 10", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(65_552)).toBe("64 KB");
    expect(formatBytes(4 * 1024 ** 2)).toBe("4.0 MB");
    expect(formatBytes(200 * 1024 ** 2)).toBe("200 MB");
    expect(formatBytes(3.25 * 1024 ** 3)).toBe("3.3 GB");
    expect(formatBytes(128 * 1024 ** 3)).toBe("128 GB");
  });
});
