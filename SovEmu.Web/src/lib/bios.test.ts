import { describe, expect, it } from "vitest";
import type { BiosFile, BiosGroup, BiosSystem, SystemBios } from "./api";
import {
  biosHref,
  biosRows,
  fileHint,
  fileTone,
  filterBiosRows,
  nextSort,
  sortBiosRows,
  statusText,
  summary,
} from "./bios";

const file = (over: Partial<BiosFile>): BiosFile => ({
  name: "scph5501.bin",
  label: "SCPH-5501 (USA)",
  state: "OK",
  foundAt: "psx/scph5501.bin",
  md5: null,
  expectedMd5: [],
  size: 524288,
  ...over,
});
const group = (over: Partial<BiosGroup>): BiosGroup => ({
  label: "BIOS",
  importance: "RECOMMENDED",
  help: "",
  anyOne: true,
  satisfied: false,
  files: [],
  ...over,
});
const bios = (state: SystemBios["state"]) => ({ bios: { state, folder: "x", groups: [] } });

describe("BIOS hints", () => {
  it("says a missing file can go anywhere, and where is tidy", () => {
    expect(fileHint(file({ state: "MISSING", foundAt: null }), "psx")).toBe(
      "Put it anywhere in the BIOS folder: a known dump under any name, or this name (bios/psx/scph5501.bin keeps it tidy).",
    );
    expect(fileHint(file({ name: "*.bin", state: "MISSING", foundAt: null }), "ps2")).toBe(
      "Put a dump anywhere in the BIOS folder (bios/ps2/ keeps it tidy).",
    );
  });

  it("says where a file was found, unless it is in the tidy place", () => {
    expect(fileHint(file({ state: "OK", foundAt: "psx/scph5501.bin" }), "psx")).toBeNull();
    expect(fileHint(file({ state: "OK", foundAt: "psx/SCPH5501.BIN" }), "psx")).toBeNull();
    expect(fileHint(file({ state: "OK", foundAt: "scph5501.bin" }), "psx")).toBe("Found at bios/scph5501.bin.");
    expect(fileHint(file({ state: "MISPLACED", foundAt: "old/scph5501.bin" }), "psx")).toBe(
      "Found at bios/old/scph5501.bin.",
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

describe("BIOS links", () => {
  it("send admins to the admin page", () => {
    expect(biosHref(false)).toBe("/bios/");
    expect(biosHref(true)).toBe("/admin/bios/");
    expect(biosHref(true, "psx")).toBe("/admin/bios/?id=psx");
  });
});

const system = (id: string, name: string, groups: BiosGroup[], state: SystemBios["state"] = "READY"): BiosSystem =>
  ({ id, name, color: "#000", bios: { state, folder: id, groups } }) as unknown as BiosSystem;

const table = () =>
  biosRows([
    system("psx", "PlayStation", [
      group({
        files: [file({}), file({ name: "scph1001.bin", label: "SCPH-1001 (USA)", state: "MISSING", foundAt: null })],
        satisfied: true,
      }),
    ]),
    { id: "snes", name: "Super Nintendo", color: "#000", bios: null } as unknown as BiosSystem,
    system(
      "lynx",
      "Lynx",
      [
        group({
          label: "Boot ROM",
          importance: "REQUIRED",
          files: [file({ name: "lynxboot.img", label: "Lynx boot ROM", size: 512, state: "MISSING", foundAt: null })],
        }),
      ],
      "MISSING",
    ),
  ]);

describe("BIOS table", () => {
  it("has a row per file of each system that uses a BIOS", () => {
    const rows = table();
    expect(rows.map((r) => `${r.systemId}:${r.file.name}`)).toEqual([
      "psx:scph5501.bin",
      "psx:scph1001.bin",
      "lynx:lynxboot.img",
    ]);
    expect(rows[2].hint).toBe(
      "Put it anywhere in the BIOS folder: a known dump under any name, or this name (bios/lynx/lynxboot.img keeps it tidy).",
    );
    expect(new Set(rows.map((r) => r.key)).size).toBe(3);
  });

  it("finds rows by system name, system id or file name", () => {
    const rows = table();
    expect(filterBiosRows(rows, "")).toHaveLength(3);
    expect(filterBiosRows(rows, "play").map((r) => r.systemId)).toEqual(["psx", "psx"]);
    expect(filterBiosRows(rows, "LYNXBOOT").map((r) => r.file.name)).toEqual(["lynxboot.img"]);
    expect(filterBiosRows(rows, "psx 1001").map((r) => r.file.name)).toEqual(["scph1001.bin"]);
    expect(filterBiosRows(rows, "SCPH-1001"), "descriptions are not searched").toHaveLength(0);
  });

  it("sorts by any column, either way, keeping ties in order", () => {
    const rows = table();
    const names = (key: Parameters<typeof sortBiosRows>[1], dir: "asc" | "desc") =>
      sortBiosRows(rows, key, dir).map((r) => r.file.name);
    expect(names("system", "asc")).toEqual(["lynxboot.img", "scph5501.bin", "scph1001.bin"]);
    expect(names("file", "desc")).toEqual(["scph5501.bin", "scph1001.bin", "lynxboot.img"]);
    expect(names("size", "asc")[0]).toBe("lynxboot.img");
    expect(names("status", "asc"), "worst first").toEqual(["lynxboot.img", "scph1001.bin", "scph5501.bin"]);
    expect(names("need", "asc")[0]).toBe("lynxboot.img");
  });

  it("flips a column's direction, and starts a new one ascending", () => {
    expect(nextSort({ key: "file", direction: "asc" }, "file")).toEqual({ key: "file", direction: "desc" });
    expect(nextSort({ key: "file", direction: "desc" }, "size")).toEqual({ key: "size", direction: "asc" });
  });

  it("says whether a missing file matters", () => {
    const [ok, notNeeded, missing] = table();
    expect(statusText(ok)).toBe("Verified");
    expect(statusText(notNeeded)).toBe("Not needed");
    expect(statusText(missing)).toBe("Missing");
  });
});
