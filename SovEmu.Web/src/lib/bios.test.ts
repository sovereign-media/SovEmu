import { describe, expect, it } from "vitest";
import type { BiosFile, BiosGroup, SystemBios } from "./api";
import { fileHint, fileTone, summary } from "./bios";

const file = (over: Partial<BiosFile>): BiosFile => ({
  name: "scph5501.bin", label: "SCPH-5501 (USA)", state: "OK", foundAt: "psx/scph5501.bin", md5: null, size: 524288, ...over,
});
const group = (over: Partial<BiosGroup>): BiosGroup => ({
  label: "BIOS", importance: "RECOMMENDED", help: "", anyOne: true, satisfied: false, files: [], ...over,
});
const bios = (state: SystemBios["state"]) => ({ bios: { state, folder: "x", groups: [] } });

describe("BIOS hints", () => {
  it("says where a missing file goes", () => {
    expect(fileHint(file({ state: "MISSING", foundAt: null }), "psx")).toBe("Put it at bios/psx/scph5501.bin");
    expect(fileHint(file({ name: "*.bin", state: "MISSING", foundAt: null }), "ps2")).toBe("Put a dump in bios/ps2/");
  });

  it("tells a wrong case from a wrong place", () => {
    expect(fileHint(file({ state: "MISPLACED", foundAt: "psx/SCPH5501.BIN" }), "psx")).toBe(
      "Found as bios/psx/SCPH5501.BIN: rename it scph5501.bin (letter case matters).",
    );
    expect(fileHint(file({ state: "MISPLACED", foundAt: "scph5501.bin" }), "psx")).toBe(
      "Found at bios/scph5501.bin: move it to bios/psx/scph5501.bin.",
    );
  });

  it("explains unknown dumps and files beside the games", () => {
    expect(fileHint(file({ state: "UNVERIFIED", md5: "abc" }), "psx")).toContain("(abc) is not a known dump");
    expect(fileHint(file({}), "psx")).toBeNull();
    expect(fileHint(file({ name: "neogeo.zip", foundAt: "roms/neogeo/neogeo.zip" }), "neogeo")).toBe(
      "Found beside the games, in roms/neogeo/neogeo.zip.",
    );
  });
});

describe("BIOS tones", () => {
  it("quietens what does not matter", () => {
    const missing = file({ state: "MISSING", foundAt: null });
    expect(fileTone(missing, group({ satisfied: true }))).toBe("quiet");
    expect(fileTone(missing, group({ importance: "OPTIONAL" }))).toBe("quiet");
    expect(fileTone(missing, group({}))).toBe("info");
    expect(fileTone(missing, group({ importance: "REQUIRED" }))).toBe("bad");
    expect(fileTone(file({ state: "MISPLACED" }), group({}))).toBe("warn");
  });
});

describe("BIOS summary", () => {
  it("counts what needs doing", () => {
    expect(summary([bios("READY"), bios("INCOMPLETE")])).toBe("Every system has what it needs.");
    expect(summary([bios("MISSING"), bios("WARNING"), bios("WARNING")])).toBe(
      "1 system is missing a required BIOS; 2 have files to check.",
    );
  });
});
