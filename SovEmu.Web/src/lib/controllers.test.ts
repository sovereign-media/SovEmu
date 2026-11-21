import { describe, expect, it } from "vitest";
import {
  boundTo,
  Capture,
  DEFAULT_KEYS,
  DEFAULT_PAD,
  defaultMapping,
  diagramKey,
  familyOf,
  identify,
  inputLabel,
  leftStickBound,
  Players,
  readHotkeys,
  rebind,
  slug,
  stickInputs,
  toBindings,
} from "./controllers";
import { bit } from "./protocol";

describe("recognising a controller (docs/CONTROLLERS.md)", () => {
  it("reads every browser's wording of the same pad", () => {
    const chrome = identify("Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)");
    const firefox = identify("045e-0b13-Xbox Wireless Controller");
    const safari = identify("45e-b13-Xbox Wireless Controller");
    for (const c of [chrome, firefox, safari])
      expect(c).toEqual({ id: "xbox", name: "Xbox Wireless Controller", family: "xbox" });
  });

  it("knows the popular models by IDs, or by name when there are none", () => {
    expect(identify("DualSense Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)").id).toBe(
      "dualsense",
    );
    expect(identify("054c-09cc-Wireless Controller")).toMatchObject({ id: "dualshock4", family: "playstation" });
    expect(identify("057e-2009-Pro Controller")).toMatchObject({ id: "switch-pro", family: "nintendo" });
    // Chrome on Windows gives XInput pads no IDs.
    expect(identify("Xbox 360 Controller (XInput STANDARD GAMEPAD)")).toEqual({
      id: "xbox",
      name: "Xbox 360 Controller",
      family: "xbox",
    });
    expect(identify("DualSense Wireless Controller")).toMatchObject({ id: "dualsense" });
  });

  it("keys any other pad by its IDs, else by its name", () => {
    expect(identify("8BitDo Pro 2 (STANDARD GAMEPAD Vendor: 2dc8 Product: 6006)")).toEqual({
      id: "pad:2dc8:6006",
      name: "8BitDo Pro 2",
      family: "generic",
    });
    // An unlisted product from a known maker keeps that maker's labels.
    expect(identify("054c-0268-PLAYSTATION(R)3 Controller")).toMatchObject({
      id: "pad:054c:0268",
      family: "playstation",
    });
    expect(identify("Generic   USB Joystick!")).toMatchObject({
      id: "pad:name:generic-usb-joystick",
      family: "generic",
    });
    expect(slug("--Ünïcode Pad--")).toBe("n-code-pad");
    expect(slug("x".repeat(60))).toHaveLength(48);
  });

  it("tells a stored ID's family, for labels when no controller is connected", () => {
    expect(familyOf("dualsense")).toBe("playstation");
    expect(familyOf("pad:057e:2017")).toBe("nintendo");
    expect(familyOf("pad:2dc8:6006")).toBe("generic");
    expect(familyOf("pad:name:arcade-stick")).toBe("generic");
  });
});

describe("labels", () => {
  it("names inputs as the controller prints them, and keys as people say them", () => {
    expect(inputLabel("south", "playstation")).toBe("✕");
    expect(inputLabel("south", "xbox")).toBe("A");
    expect(inputLabel("south", "nintendo")).toBe("B");
    expect(inputLabel("l2", "nintendo")).toBe("ZL");
    expect(inputLabel("KeyZ", null)).toBe("Z");
    expect(inputLabel("ArrowUp", null)).toBe("↑");
    expect(inputLabel("ShiftLeft", null)).toBe("Left Shift");
    expect(boundTo(DEFAULT_PAD, "B", "playstation")).toEqual(["✕"]);
    expect(boundTo(DEFAULT_KEYS, "SELECT", null)).toEqual(["Left Shift", "Right Shift"]);
  });
});

describe("mappings", () => {
  it("defaults by position, and the keyboard to its own keys", () => {
    expect(defaultMapping("xbox").south).toBe("B");
    expect(defaultMapping("keyboard").KeyZ).toBe("B");
    const m = defaultMapping("xbox");
    m.south = "A";
    expect(DEFAULT_PAD.south).toBe("B");
  });

  it("gives a rebound button just its new input, which drives nothing else", () => {
    const swapped = rebind(DEFAULT_PAD, "east", "B");
    expect(swapped.east).toBe("B");
    expect(swapped.south).toBeUndefined();
    expect(Object.values(swapped).filter((b) => b === "A")).toEqual([]);
    expect(rebind(DEFAULT_KEYS, "KeyJ", "SELECT")).toMatchObject({ KeyJ: "SELECT" });
    expect(rebind(DEFAULT_KEYS, "KeyJ", "SELECT").ShiftLeft).toBeUndefined();
    expect(toBindings({ south: "A", east: "B" })).toEqual([
      { input: "east", button: "B" },
      { input: "south", button: "A" },
    ]);
  });
});

describe("hotkeys", () => {
  it("read SELECT with R or L after mapping, and keep them from the game", () => {
    const ff = readHotkeys(bit("SELECT") | bit("R") | bit("B"));
    expect(ff).toEqual({ state: bit("B"), fastForward: true, rewind: false });
    expect(readHotkeys(bit("SELECT") | bit("L")).rewind).toBe(true);
    expect(readHotkeys(bit("SELECT") | bit("START"))).toEqual({
      state: bit("SELECT") | bit("START"),
      fastForward: false,
      rewind: false,
    });
  });
});

describe("players", () => {
  it("gives each controller the lowest free player, and frees it when it goes", () => {
    const p = new Players<string>();
    expect(p.join("a")).toBe(0);
    expect(p.join("b")).toBe(1);
    expect(p.join("a")).toBe(0);
    p.leave("a");
    expect(p.join("c")).toBe(0);
    expect(p.join("d")).toBe(2);
    expect(p.join("e")).toBe(3);
    expect(p.join("f")).toBe(3); // a fifth shares the last player
    p.set("b", 9);
    expect(p.get("b")).toBe(3);
  });
});

describe("remapping (tap Back to cancel, hold it to bind it)", () => {
  const held = (...inputs: string[]) => new Set(inputs);

  it("binds any other new input at once", () => {
    const c = new Capture("south", [], 0);
    expect(c.update(held(), 10)).toBeUndefined();
    expect(c.update(held("east"), 20)).toBe("east");
  });

  it("cancels on a tap of Back, and binds Back when it is held for a second", () => {
    const tap = new Capture("south", [], 0);
    expect(tap.update(held("south"), 100)).toBeUndefined();
    expect(tap.update(held(), 400)).toBeNull();
    const hold = new Capture("south", [], 0);
    expect(hold.update(held("south"), 100)).toBeUndefined();
    expect(hold.update(held("south"), 900)).toBeUndefined();
    expect(hold.update(held("south"), 1100)).toBe("south");
  });

  it("ignores what was already held, cancels on guide, and gives up after a while", () => {
    const c = new Capture("Escape", ["Enter"], 0);
    expect(c.update(held("Enter"), 50)).toBeUndefined();
    expect(c.update(held(), 60)).toBeUndefined();
    expect(c.update(held("Enter"), 70)).toBe("Enter");
    expect(new Capture("south", [], 0).update(held("guide"), 10)).toBeNull();
    // Back held from the start (the press that opened the menu) does not count as a tap.
    const opened = new Capture("south", ["south"], 0);
    expect(opened.update(held(), 10)).toBeUndefined();
    expect(new Capture("south", [], 0).update(held(), 8000)).toBeNull();
  });
});

describe("stick directions", () => {
  it("reads directions past half-way, on both sticks", () => {
    expect(stickInputs(-0.9, 0.1, 0, 0.8)).toEqual(["ls_left", "rs_down"]);
    expect(stickInputs(0.4, -0.4, 0, 0)).toEqual([]);
  });
  it("labels them and lights their stick", () => {
    expect(inputLabel("ls_left", "xbox")).toBe("LS ←");
    expect(inputLabel("rs_up", "playstation")).toBe("RS ↑");
    expect(diagramKey("rs_up")).toBe("r3");
    expect(diagramKey("south")).toBe("south");
  });
  it("knows when the left stick is bound (and so stops driving the D-pad)", () => {
    expect(leftStickBound({ ...DEFAULT_PAD })).toBe(false);
    expect(leftStickBound({ ...DEFAULT_PAD, ls_left: "L" })).toBe(true);
    expect(leftStickBound({ rs_up: "X" })).toBe(false);
  });
});
