import { describe, expect, it } from "vitest";
import { systemPhoto } from "./systemPhotos";

describe("system photos", () => {
  it("has a photo for every system SovEmu knows, by its id", () => {
    const ids = "nes snes gb gbc gba megadrive mastersystem psx fbneo neogeo gamegear n64 dreamcast gamecube ps2";
    for (const id of ids.split(" ")) expect(systemPhoto(id), id).toBeTruthy();
  });

  it("has none for a system it does not know", () => {
    expect(systemPhoto("vectrex")).toBeUndefined();
  });
});
