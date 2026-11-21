import { describe, expect, it } from "vitest";
import type { CoreSetting } from "./api";
import { choiceLabel, filterSettings, groupSettings, guessGroup, runAheadChoices, sourceBadge } from "./settings";

const s = (key: string, description: string, over: Partial<CoreSetting> = {}): CoreSetting => ({
  key, description, info: "", category: null, value: "off", default: "off", choices: ["off", "on"], labels: ["Off", "On"],
  source: "CORE", ...over,
});

describe("emulator settings", () => {
  const all = [
    s("mupen64plus-rdp-plugin", "RDP Plugin", { value: "gliden64" }),
    s("mupen64plus-43screensize", "4:3 Resolution", { info: "Internal resolution for 4:3 games." }),
  ];

  it("finds settings by every word, in description, explanation, key or value", () => {
    expect(filterSettings(all, "")).toHaveLength(2);
    expect(filterSettings(all, "rdp").map((x) => x.key)).toEqual(["mupen64plus-rdp-plugin"]);
    expect(filterSettings(all, "plugin gliden")).toHaveLength(1);
    expect(filterSettings(all, "internal")).toHaveLength(1);
    expect(filterSettings(all, "resolution rdp")).toHaveLength(0);
  });

  it("labels where a value comes from", () => {
    expect(sourceBadge("CORE")).toBeNull();
    expect(sourceBadge("ADMIN")?.text).toBe("Changed");
    expect(sourceBadge("ENVIRONMENT")?.text).toBe("Environment");
  });

  it("shows the core's labels and marks the default", () => {
    const setting = s("x", "X");
    expect(choiceLabel(setting, "off")).toBe("Off (default)");
    expect(choiceLabel(setting, "on")).toBe("On");
    expect(choiceLabel({ ...setting, labels: [] }, "on")).toBe("on");
    expect(choiceLabel(setting, "custom")).toBe("custom");
  });
});

describe("grouping", () => {
  it("guesses a group from the key and description", () => {
    expect(guessGroup(s("snes9x_audio_interpolation", "Audio Interpolation"))).toBe("Audio");
    expect(guessGroup(s("fceumm_apu_1", "Channel 1 (Square 1)"))).toBe("Audio");
    expect(guessGroup(s("snes9x_lightgun_mode", "Light Gun Mode"))).toBe("Input");
    expect(guessGroup(s("snes9x_aspect", "Preferred Aspect Ratio"))).toBe("Video");
    expect(guessGroup(s("snes9x_overclock_superfx", "SuperFX Overclocking"))).toBe("Performance and hacks");
    expect(guessGroup(s("snes9x_region", "Console Region (Reload Core)"))).toBe("System");
    expect(guessGroup(s("core_thing", "Something else"))).toBe("Other");
  });

  it("uses the core's categories first, then SovEmu's guesses for the rest", () => {
    const groups = groupSettings(
      [
        s("a_region", "Region", { category: "system" }),
        s("a_palette", "Colour Palette", { category: "video" }),
        s("a_volume", "Master Volume"),
        s("a_mystery", "Mystery"),
        s("a_aspect", "Aspect Ratio"),
        s("a_crop", "Crop Overscan", { category: "nope" }),
      ],
      [
        { key: "video", description: "Video", info: "" },
        { key: "system", description: "System", info: "The console." },
        { key: "empty", description: "Unused", info: "" },
      ],
    );
    expect(groups.map((g) => [g.title, g.settings.map((x) => x.key)])).toEqual([
      // System, Video, Audio, Input first, whoever named them; a guess joins a core category of
      // the same name.
      ["System", ["a_region"]],
      ["Video", ["a_palette", "a_aspect", "a_crop"]],
      ["Audio", ["a_volume"]],
      ["Other", ["a_mystery"]],
    ]);
    expect(groups[0].info).toBe("The console.");
  });

  it("puts the core's other categories after the main four, then SovEmu's other guesses", () => {
    const groups = groupSettings(
      [s("h", "Hack", { category: "hacks" }), s("r", "Region"), s("o", "Overclock"), s("l", "Light gun", { category: "gun" })],
      [
        { key: "hacks", description: "Emulation Hacks", info: "" },
        { key: "gun", description: "Light Gun", info: "" },
      ],
    );
    expect(groups.map((g) => g.title)).toEqual(["System", "Emulation Hacks", "Light Gun", "Performance and hacks"]);
  });

  it("guesses everything for a core without categories", () => {
    const groups = groupSettings([s("x_volume", "Volume"), s("x_res", "Internal Resolution")], []);
    expect(groups.map((g) => g.title)).toEqual(["Video", "Audio"]);
  });
});

describe("runAheadChoices", () => {
  it("offers off, then each frame with the lag it hides", () => {
    expect(runAheadChoices(2)).toEqual([
      { value: "0", label: "Off" },
      { value: "1", label: "1 frame (~17 ms)" },
      { value: "2", label: "2 frames (~33 ms)" },
    ]);
  });
});
