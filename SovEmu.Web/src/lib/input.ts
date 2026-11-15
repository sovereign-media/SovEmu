// Keyboard and gamepads as RetroPad state per player, plus the emulator hotkeys.
//
// Each controller is recognised by model (src/lib/controllers.ts, docs/CONTROLLERS.md) and
// drives its own player in the order controllers connect; the keyboard is player 1. Mappings
// are the user's, per model, saved on the server and shared with the apps; a model without one
// uses the defaults. Hotkeys follow console frontends: on a keyboard, hold Tab to fast-forward
// and Backspace to rewind; on a gamepad, hold SELECT with R (fast-forward) or L (rewind), read
// after mapping, and those buttons are kept from the game while the combo is held.
import {
  type ControllerInfo,
  defaultMapping,
  identify,
  KEYBOARD,
  type Mapping,
  PAD_INPUTS,
  Players,
  readHotkeys,
  RESERVED_KEYS,
} from "./controllers";
import { type Analog, bit, CENTRED, type Hotkeys, MAX_PORTS } from "./protocol";

const STICK_THRESHOLD = 0.5;
const FAST_FORWARD_KEY = "Tab";
const REWIND_KEY = "Backspace";
/** Stick travel ignored around the centre: worn sticks rest a little off it. */
const DEADZONE = 0.15;
/** Standard gamepad trigger buttons, whose `value` is the analog trigger. */
const PAD_L2 = 6;
const PAD_R2 = 7;

/** A gamepad axis (−1..1) as a libretro stick value, with the dead zone taken out. */
export function axisToStick(v: number): number {
  const magnitude = Math.abs(v);
  if (magnitude < DEADZONE) return 0;
  const scaled = (magnitude - DEADZONE) / (1 - DEADZONE);
  return Math.sign(v) * Math.round(Math.min(1, scaled) * 32767);
}

/** A trigger (0..1) as a libretro analog button value. */
export const triggerValue = (v: number) => Math.round(Math.max(0, Math.min(1, v)) * 32767);

export interface InputReading {
  /** RetroPad state per port, from port 0 to the highest port in use. */
  ports: number[];
  hotkeys: Hotkeys;
  /** Sticks and triggers per port, as long as `ports`. */
  analog: Analog[];
}

/** A connected input device, for the controls menu. */
export interface Device {
  /** `keyboard`, or `pad:<Gamepad.index>`: this connection, not the model. */
  key: string;
  info: ControllerInfo;
  port: number;
}

/** A gamepad's state through its mapping: the RetroPad bits it holds. */
export function padState(pressed: (i: number) => boolean, mapping: Mapping): number {
  let state = 0;
  PAD_INPUTS.forEach((input, i) => {
    const button = mapping[input];
    if (button && pressed(i)) state |= bit(button);
  });
  return state;
}

export class InputState {
  /**
   * The game reads analog sticks (a 3D system): the left stick moves the stick, not the D-pad.
   * Otherwise the left stick doubles as a D-pad, as it does in 2D frontends.
   */
  analogSticks = false;
  /** The user's saved mappings by controller ID; anything missing uses the defaults. */
  private saved = new Map<string, Mapping>();
  /** Gamepads' players; the keyboard does not take a slot, so the first pad is player 1 too. */
  private players = new Players<string>();
  private keyboardPort = 0;
  private known = new Map<string, ControllerInfo>();
  private held = new Set<string>();

  constructor(target: Window = window) {
    target.addEventListener("keydown", (e) => this.onKey(e, true));
    target.addEventListener("keyup", (e) => this.onKey(e, false));
    target.addEventListener("blur", () => this.held.clear());
    target.addEventListener("gamepadconnected", (e) => this.players.join(`pad:${e.gamepad.index}`));
    target.addEventListener("gamepaddisconnected", (e) => this.players.leave(`pad:${e.gamepad.index}`));
    // Pads already connected (a reload) join in slot order.
    for (const pad of this.pads()) this.players.join(`pad:${pad.index}`);
  }

  setMappings(mappings: { controller: string; bindings: { input: string; button: string }[] }[]) {
    this.saved = new Map(
      mappings.map((m) => [m.controller, Object.fromEntries(m.bindings.map((b) => [b.input, b.button])) as Mapping]),
    );
  }

  /** The mapping in force for a controller model: the user's, else the defaults. */
  mapping(controller: string): Mapping {
    return this.saved.get(controller) ?? defaultMapping(controller);
  }

  /** Records a saved (or, with null, reset) mapping. */
  remember(controller: string, mapping: Mapping | null) {
    if (mapping) this.saved.set(controller, mapping);
    else this.saved.delete(controller);
  }

  /** A gamepad's model, from its `Gamepad.id` (cached: read() runs every frame). */
  private identify(gamepadId: string): ControllerInfo {
    let info = this.known.get(gamepadId);
    if (!info) this.known.set(gamepadId, (info = identify(gamepadId)));
    return info;
  }

  private pads(): Gamepad[] {
    return (navigator.getGamepads ? [...navigator.getGamepads()] : []).filter((p): p is Gamepad => !!p);
  }

  /** The keyboard and every connected gamepad, with their players. */
  devices(): Device[] {
    const pads = this.pads().map((pad) => {
      const key = `pad:${pad.index}`;
      return { key, info: this.identify(pad.id), port: this.players.join(key) };
    });
    const keyboard = { id: KEYBOARD, name: "Keyboard", family: "generic" as const };
    return [{ key: KEYBOARD, info: keyboard, port: this.keyboardPort }, ...pads];
  }

  setPort(device: string, port: number) {
    if (device === KEYBOARD) this.keyboardPort = Math.max(0, Math.min(MAX_PORTS - 1, port));
    else this.players.set(device, port);
  }

  private onKey(e: KeyboardEvent, down: boolean) {
    const relevant = e.code in this.mapping(KEYBOARD) || e.code === FAST_FORWARD_KEY || e.code === REWIND_KEY;
    if (!relevant) return;
    if (down) this.held.add(e.code);
    else this.held.delete(e.code);
    e.preventDefault();
  }

  /** The current state of every player. */
  read(): InputReading {
    const ports = new Array<number>(MAX_PORTS).fill(0);
    const analog: Analog[] = Array.from({ length: MAX_PORTS }, () => ({ ...CENTRED }));
    const hotkeys: Hotkeys = { fastForward: this.held.has(FAST_FORWARD_KEY), rewind: this.held.has(REWIND_KEY) };
    let used = 0;

    const keys = this.mapping(KEYBOARD);
    const keyboardPort = this.keyboardPort;
    for (const code of this.held) {
      const name = keys[code];
      if (name) ports[keyboardPort] |= bit(name);
    }
    used = Math.max(used, keyboardPort);

    const analogSet = new Set<number>();
    for (const pad of this.pads()) {
      const port = this.players.join(`pad:${pad.index}`);
      const pressed = (i: number) => !!pad.buttons[i]?.pressed;
      const read = readHotkeys(padState(pressed, this.mapping(this.identify(pad.id).id)));
      hotkeys.fastForward ||= read.fastForward;
      hotkeys.rewind ||= read.rewind;
      let state = read.state;
      if (!analogSet.has(port)) {
        analogSet.add(port);
        const [lx = 0, ly = 0, rx = 0, ry = 0] = pad.axes;
        analog[port] = {
          lx: axisToStick(lx), ly: axisToStick(ly), rx: axisToStick(rx), ry: axisToStick(ry),
          l2: triggerValue(pad.buttons[PAD_L2]?.value ?? 0), r2: triggerValue(pad.buttons[PAD_R2]?.value ?? 0),
        };
      }
      if (!this.analogSticks) {
        const [x = 0, y = 0] = pad.axes;
        if (x < -STICK_THRESHOLD) state |= bit("LEFT");
        if (x > STICK_THRESHOLD) state |= bit("RIGHT");
        if (y < -STICK_THRESHOLD) state |= bit("UP");
        if (y > STICK_THRESHOLD) state |= bit("DOWN");
      }
      ports[port] |= state;
      used = Math.max(used, port);
    }
    return { ports: ports.slice(0, used + 1), hotkeys, analog: analog.slice(0, used + 1) };
  }
}

/**
 * Resolves with the next input pressed on one device, for remapping: a `KeyboardEvent.code` on
 * the keyboard (not the hotkeys), else a positional input name on that gamepad. Escape cancels,
 * and so does waiting `timeoutMs`.
 */
export function captureInput(device: string, timeoutMs = 8000): Promise<string | null> {
  return new Promise((resolve) => {
    const index = device.startsWith("pad:") ? Number(device.slice(4)) : null;
    const before = new Set<number>();
    const pad = () => (index === null ? null : navigator.getGamepads?.()[index] ?? null);
    pad()?.buttons.forEach((b, i) => b.pressed && before.add(i));
    let raf = 0;
    const timer = setTimeout(() => done(null), timeoutMs);
    const done = (result: string | null) => {
      cancelAnimationFrame(raf);
      clearTimeout(timer);
      removeEventListener("keydown", onKey, true);
      resolve(result);
    };
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopImmediatePropagation();
      if (e.code === "Escape") return done(null);
      if (index === null && !RESERVED_KEYS.includes(e.code)) done(e.code);
    };
    addEventListener("keydown", onKey, true);
    const poll = () => {
      const p = pad();
      if (p) {
        for (let i = 0; i < Math.min(p.buttons.length, PAD_INPUTS.length); i++) {
          if (p.buttons[i].pressed && !before.has(i)) return done(PAD_INPUTS[i]);
          if (!p.buttons[i].pressed) before.delete(i);
        }
      }
      raf = requestAnimationFrame(poll);
    };
    raf = requestAnimationFrame(poll);
  });
}
