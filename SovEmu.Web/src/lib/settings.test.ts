import { describe, expect, it } from "vitest";
import type { CoreSetting } from "./api";
import { choiceLabel, filterSettings, sourceBadge } from "./settings";

const s = (key: string, description: string, value = "off"): CoreSetting => ({
  key, description, value, default: "off", choices: ["off", "on"], source: "core",
});

describe("emulator settings", () => {
  const all = [s("mupen64plus-rdp-plugin", "RDP Plugin", "gliden64"), s("mupen64plus-43screensize", "4:3 Resolution")];

  it("finds settings by every word, in description, key or value", () => {
    expect(filterSettings(all, "")).toHaveLength(2);
    expect(filterSettings(all, "rdp").map((x) => x.key)).toEqual(["mupen64plus-rdp-plugin"]);
    expect(filterSettings(all, "plugin gliden")).toHaveLength(1);
    expect(filterSettings(all, "resolution rdp")).toHaveLength(0);
  });

  it("labels where a value comes from", () => {
    expect(sourceBadge("core")).toBeNull();
    expect(sourceBadge("admin")?.text).toBe("Changed");
    expect(sourceBadge("environment")?.text).toBe("Environment");
    expect(choiceLabel("off", "off")).toBe("off (default)");
    expect(choiceLabel("on", "off")).toBe("on");
  });
});
