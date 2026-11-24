// Controllers as docs/CONTROLLERS.md describes them, shared with the Android and iOS apps: which
// model a gamepad is (its ID, the key for the user's saved mapping), its family (for button
// labels), the positional input names, the default mappings, hotkeys and player slots. Plain
// TypeScript, no DOM, so Vitest covers it.
import { type ButtonName, bit, MAX_PORTS } from "./protocol";

/** A controller's inputs by position: the W3C standard gamepad's buttons 0-15, in order. */
export const PAD_INPUTS = [
  "south",
  "east",
  "west",
  "north",
  "l1",
  "r1",
  "l2",
  "r2",
  "select",
  "start",
  "l3",
  "r3",
  "up",
  "down",
  "left",
  "right",
] as const;
export type PadInput = (typeof PAD_INPUTS)[number];

/** A controller's stick directions: a stick pushed past half-way, bindable like buttons. */
export const STICK_INPUTS = [
  "ls_up",
  "ls_down",
  "ls_left",
  "ls_right",
  "rs_up",
  "rs_down",
  "rs_left",
  "rs_right",
] as const;
export type StickInput = (typeof STICK_INPUTS)[number];
const ARROWS = { up: "↑", down: "↓", left: "←", right: "→" } as const;

/** The left stick drives the D-pad unless the mapping binds any of its directions. */
export const leftStickBound = (mapping: Mapping): boolean => Object.keys(mapping).some((i) => i.startsWith("ls_"));

/** The stick directions (`ls_left` …) held at axis values -1..1, past half-way. */
export function stickInputs(lx: number, ly: number, rx: number, ry: number, threshold = 0.5): StickInput[] {
  const out: StickInput[] = [];
  const add = (side: "ls" | "rs", x: number, y: number) => {
    if (x < -threshold) out.push(`${side}_left`);
    if (x > threshold) out.push(`${side}_right`);
    if (y < -threshold) out.push(`${side}_up`);
    if (y > threshold) out.push(`${side}_down`);
  };
  add("ls", lx, ly);
  add("rs", rx, ry);
  return out;
}

/** The diagram key an input lights: a stick direction lights its stick. */
export const diagramKey = (input: string): string =>
  input.startsWith("ls_") ? "l3" : input.startsWith("rs_") ? "r3" : input;

/**
 * One of the player's sticks, as a system's analog controls name it: what to call it, and the
 * controller diagram's key it lights. Sticks are not remapped (docs/CONTROLLERS.md), so a stick's
 * row only says which stick drives the control.
 */
export const stickRow = (stick: "LEFT" | "RIGHT"): { name: string; diagramKey: string } =>
  stick === "LEFT" ? { name: "Left stick", diagramKey: "l3" } : { name: "Right stick", diagramKey: "r3" };

export type Family = "xbox" | "playstation" | "nintendo" | "generic";

/** Input → RetroPad button. An input drives one button; a button may have several inputs. */
export type Mapping = Record<string, ButtonName>;

export const KEYBOARD = "keyboard";
/** The keyboard's fast-forward and rewind keys: never bound. */
export const RESERVED_KEYS = ["Tab", "Backspace"];

export const DEFAULT_PAD: Mapping = {
  south: "B",
  east: "A",
  west: "Y",
  north: "X",
  l1: "L",
  r1: "R",
  l2: "L2",
  r2: "R2",
  select: "SELECT",
  start: "START",
  l3: "L3",
  r3: "R3",
  up: "UP",
  down: "DOWN",
  left: "LEFT",
  right: "RIGHT",
};

export const DEFAULT_KEYS: Mapping = {
  ArrowUp: "UP",
  ArrowDown: "DOWN",
  ArrowLeft: "LEFT",
  ArrowRight: "RIGHT",
  KeyZ: "B",
  KeyX: "A",
  KeyA: "Y",
  KeyS: "X",
  KeyQ: "L",
  KeyW: "R",
  Enter: "START",
  ShiftLeft: "SELECT",
  ShiftRight: "SELECT",
};

export const defaultMapping = (controller: string): Mapping => ({
  ...(controller === KEYBOARD ? DEFAULT_KEYS : DEFAULT_PAD),
});

// --- Recognising a controller -----------------------------------------------------------------

const MODELS: { id: string; name: string; family: Family; ids: string[] }[] = [
  {
    id: "xbox",
    name: "Xbox Controller",
    family: "xbox",
    ids: [
      "028e",
      "0291",
      "0719",
      "02d1",
      "02dd",
      "02e0",
      "02e3",
      "02ea",
      "02fd",
      "0b00",
      "0b05",
      "0b12",
      "0b13",
      "0b20",
      "0b22",
    ].map((p) => `045e:${p}`),
  },
  { id: "dualsense", name: "DualSense", family: "playstation", ids: ["054c:0ce6", "054c:0df2"] },
  { id: "dualshock4", name: "DualShock 4", family: "playstation", ids: ["054c:05c4", "054c:09cc", "054c:0ba0"] },
  { id: "switch-pro", name: "Switch Pro Controller", family: "nintendo", ids: ["057e:2009"] },
];

const VENDOR_FAMILY: Record<string, Family> = { "045e": "xbox", "054c": "playstation", "057e": "nintendo" };

/** The keyboard and the popular models, for lists of controllers to set up. */
export const KNOWN_CONTROLLERS: { id: string; name: string; family: Family }[] = [
  { id: KEYBOARD, name: "Keyboard", family: "generic" },
  ...MODELS.map(({ id, name, family }) => ({ id, name, family })),
];

/** A controller ID's family, for labels: its model's, else its vendor's, else generic. */
export function familyOf(id: string): Family {
  const model = MODELS.find((m) => m.id === id);
  if (model) return model.family;
  return VENDOR_FAMILY[/^pad:([0-9a-f]{4}):/.exec(id)?.[1] ?? ""] ?? "generic";
}

/** The console's buttons (libretro's RetroPad), in the order menus list them, with names. */
export const PAD_BUTTONS: [ButtonName, string][] = [
  ["UP", "Up"],
  ["DOWN", "Down"],
  ["LEFT", "Left"],
  ["RIGHT", "Right"],
  ["A", "A"],
  ["B", "B"],
  ["X", "X"],
  ["Y", "Y"],
  ["L", "L"],
  ["R", "R"],
  ["L2", "L2"],
  ["R2", "R2"],
  ["L3", "L3 (left stick)"],
  ["R3", "R3 (right stick)"],
  ["START", "Start"],
  ["SELECT", "Select"],
];

/** Where each face button sits, since labels differ between makers. */
export const POSITIONS: Partial<Record<PadInput, string>> = {
  south: "bottom",
  east: "right",
  west: "left",
  north: "top",
};

export interface ControllerInfo {
  /** The mapping key: `xbox`, `dualsense` …, `pad:<vendor>:<product>` or `pad:name:<slug>`. */
  id: string;
  /** What the controller calls itself, without the browser's additions. */
  name: string;
  family: Family;
}

/** A name as an ID part: lowercase, other runs a `-`, at most 48 characters. */
export const slug = (s: string) =>
  s
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48)
    .replace(/-+$/, "");

const hex4 = (s: string) => s.toLowerCase().padStart(4, "0");

/**
 * Reads a Gamepad API `id`, worded differently by each browser:
 * Chrome `Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)`,
 * Firefox `045e-0b13-Xbox Wireless Controller`, Safari `45e-b13-Xbox Wireless Controller`,
 * and Chrome on Windows `Xbox 360 Controller (XInput STANDARD GAMEPAD)` (no IDs).
 */
export function identify(gamepadId: string): ControllerInfo {
  let vendor: string | null = null;
  let product: string | null = null;
  let name = gamepadId.trim();
  const chrome = /\(.*Vendor:\s*([0-9a-f]{1,4})\s+Product:\s*([0-9a-f]{1,4})\s*\)/i.exec(name);
  const prefixed = /^([0-9a-f]{1,4})-([0-9a-f]{1,4})-(.*)$/i.exec(name);
  if (chrome) {
    [vendor, product] = [hex4(chrome[1]), hex4(chrome[2])];
    name = name.slice(0, chrome.index).trim();
  } else if (prefixed) {
    [vendor, product, name] = [hex4(prefixed[1]), hex4(prefixed[2]), prefixed[3].trim()];
  }
  const xinput = /xinput/i.test(name);
  name = name.replace(/\s*\((?:XInput )?STANDARD GAMEPAD\)\s*$/i, "").trim() || gamepadId;
  const lower = name.toLowerCase();

  const ids = vendor && product ? `${vendor}:${product}` : null;
  const byIds = ids ? MODELS.find((m) => m.ids.includes(ids)) : undefined;
  const byName =
    xinput || /xbox/.test(lower)
      ? MODELS[0]
      : /dualsense/.test(lower)
        ? MODELS[1]
        : /dualshock 4/.test(lower) || (vendor === "054c" && /wireless controller/.test(lower))
          ? MODELS[2]
          : vendor === "057e" && /pro controller/.test(lower)
            ? MODELS[3]
            : undefined;
  // Known IDs win; a name only names the model when the IDs are unknown or missing.
  const model = byIds ?? (ids && VENDOR_FAMILY[vendor!] === undefined ? undefined : byName);
  if (model) return { id: model.id, name, family: model.family };
  const family: Family =
    (vendor && VENDOR_FAMILY[vendor]) ||
    (/playstation|dualshock|ps[345]/.test(lower)
      ? "playstation"
      : /nintendo|switch|joy-con/.test(lower)
        ? "nintendo"
        : "generic");
  return { id: ids ? `pad:${ids}` : `pad:name:${slug(name) || "unknown"}`, name, family };
}

// --- Labels -----------------------------------------------------------------------------------

const XBOX: Record<PadInput, string> = {
  south: "A",
  east: "B",
  west: "X",
  north: "Y",
  l1: "LB",
  r1: "RB",
  l2: "LT",
  r2: "RT",
  select: "View",
  start: "Menu",
  l3: "LS",
  r3: "RS",
  up: "D-pad ↑",
  down: "D-pad ↓",
  left: "D-pad ←",
  right: "D-pad →",
};
const LABELS: Record<Family, Record<PadInput, string>> = {
  xbox: XBOX,
  generic: XBOX,
  playstation: {
    ...XBOX,
    south: "✕",
    east: "○",
    west: "□",
    north: "△",
    l1: "L1",
    r1: "R1",
    l2: "L2",
    r2: "R2",
    select: "Create",
    start: "Options",
    l3: "L3",
    r3: "R3",
  },
  nintendo: {
    ...XBOX,
    south: "B",
    east: "A",
    west: "Y",
    north: "X",
    l1: "L",
    r1: "R",
    l2: "ZL",
    r2: "ZR",
    select: "−",
    start: "+",
  },
};

/** An input as its controller labels it ("✕", "LB"), or a key as a person would name it. */
export function inputLabel(input: string, family: Family | null): string {
  if (family && input in LABELS[family]) return LABELS[family][input as PadInput];
  const stick = /^(ls|rs)_(up|down|left|right)$/.exec(input);
  if (stick) return `${stick[1].toUpperCase()} ${ARROWS[stick[2] as keyof typeof ARROWS]}`;
  return input
    .replace(/^Key/, "")
    .replace(/^Digit/, "")
    .replace(/^Arrow(.*)$/, (_, d: string) => ({ Up: "↑", Down: "↓", Left: "←", Right: "→" })[d] ?? d)
    .replace(/^(Shift|Control|Alt|Meta)(Left|Right)$/, (_, k: string, side: string) => `${side} ${k}`);
}

/** The inputs bound to `button`, labelled. */
export function boundTo(mapping: Mapping, button: ButtonName, family: Family | null): string[] {
  return Object.entries(mapping)
    .filter(([, b]) => b === button)
    .map(([input]) => inputLabel(input, family));
}

/**
 * Binds `input` to `button`. Like a console frontend, a button then has just that input on
 * this controller: what drove it before is unbound, and `input` stops driving anything else.
 */
export function rebind(mapping: Mapping, input: string, button: ButtonName): Mapping {
  const next = Object.fromEntries(Object.entries(mapping).filter(([i, b]) => b !== button && i !== input));
  return { ...next, [input]: button };
}

/** The API's binding list as a mapping. */
export const toMapping = (bindings: { input: string; button: string }[]): Mapping =>
  Object.fromEntries(bindings.map((b) => [b.input, b.button])) as Mapping;

/** A mapping as the API's binding list, in a stable order. */
export const toBindings = (m: Mapping) =>
  Object.entries(m)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([input, button]) => ({ input, button }));

// --- Hotkeys ----------------------------------------------------------------------------------

/**
 * A port's state with the gamepad hotkeys read out of it, after mapping: SELECT+R fast-forwards
 * and SELECT+L rewinds, and those buttons are kept from the game while the combo is held.
 */
export function readHotkeys(state: number): { state: number; fastForward: boolean; rewind: boolean } {
  const held = (b: ButtonName) => (state & bit(b)) !== 0;
  const fastForward = held("SELECT") && held("R");
  const rewind = held("SELECT") && held("L");
  const masked = fastForward || rewind ? state & ~(bit("SELECT") | bit("L") | bit("R")) : state;
  return { state: masked, fastForward, rewind };
}

// --- Players ----------------------------------------------------------------------------------

/**
 * Which player (port) each controller drives: a new one takes the lowest free port, a gone one
 * frees it, and a person can move one by hand. Several may share a port.
 */
export class Players<K> {
  private ports = new Map<K, number>();

  /** The controller's port, assigning the lowest free one the first time it is seen. */
  join(key: K): number {
    const known = this.ports.get(key);
    if (known !== undefined) return known;
    const taken = new Set(this.ports.values());
    let port = 0;
    while (taken.has(port) && port < MAX_PORTS - 1) port++;
    this.ports.set(key, port);
    return port;
  }

  leave(key: K) {
    this.ports.delete(key);
  }

  get(key: K): number | undefined {
    return this.ports.get(key);
  }

  set(key: K, port: number) {
    this.ports.set(key, Math.max(0, Math.min(MAX_PORTS - 1, port)));
  }
}

// --- Remapping --------------------------------------------------------------------------------

export const HOLD_TO_BIND_MS = 1000;
export const CAPTURE_TIMEOUT_MS = 8000;
/** The standard gamepad's Home/guide button, which cancels a capture. */
export const GUIDE = "guide";

/**
 * Waiting for the input to bind (docs/CONTROLLERS.md, "Remapping"): any new input binds at
 * once, except the menu's Back input (`back`), which cancels on a tap and binds when held for a
 * second; guide cancels, and so does a wait with no new input. Inputs held at the start are
 * ignored until released. Fed the held inputs and the time; plain, so it can be tested.
 */
export class Capture {
  private ignored: Set<string>;
  private backSince: number | null = null;

  constructor(
    private readonly back: string,
    held: Iterable<string>,
    private readonly start: number,
  ) {
    this.ignored = new Set(held);
  }

  /** The input to bind, null to cancel, or undefined to keep waiting. */
  update(held: Set<string>, now: number): string | null | undefined {
    for (const input of [...this.ignored]) if (!held.has(input)) this.ignored.delete(input);
    if (this.backSince !== null) {
      if (!held.has(this.back)) return null;
      if (now - this.backSince >= HOLD_TO_BIND_MS) return this.back;
    }
    for (const input of held) {
      if (this.ignored.has(input) || input === this.back) continue;
      return input === GUIDE ? null : input;
    }
    if (this.backSince === null && held.has(this.back) && !this.ignored.has(this.back)) this.backSince = now;
    if (this.backSince === null && now - this.start >= CAPTURE_TIMEOUT_MS) return null;
    return undefined;
  }
}
