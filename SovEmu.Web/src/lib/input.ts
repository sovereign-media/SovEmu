// Keyboard and gamepad state as a RetroPad bitmask for port 0, plus the emulator hotkeys.
//
// Bindings are remappable and kept per browser (localStorage). Hotkeys follow console frontends:
// on a keyboard, hold Tab to fast-forward and Backspace to rewind; on a gamepad, hold Select with
// R (fast-forward) or L (rewind), and those buttons are kept from the game while the combo is held.
import { bit, type ButtonName, type Hotkeys } from "./protocol";

export interface Bindings {
  /** KeyboardEvent.code → RetroPad button. */
  keys: Record<string, ButtonName>;
  /** W3C standard gamepad button index → RetroPad button. */
  pad: Record<number, ButtonName>;
}

export const DEFAULT_BINDINGS: Bindings = {
  keys: {
    ArrowUp: "UP", ArrowDown: "DOWN", ArrowLeft: "LEFT", ArrowRight: "RIGHT",
    KeyZ: "B", KeyX: "A", KeyA: "Y", KeyS: "X", KeyQ: "L", KeyW: "R",
    Enter: "START", ShiftLeft: "SELECT", ShiftRight: "SELECT",
  },
  // By position: bottom face = B, right face = A, left face = Y, top face = X (the SNES layout
  // libretro's RetroPad is modelled on).
  pad: {
    0: "B", 1: "A", 2: "Y", 3: "X", 4: "L", 5: "R", 6: "L2", 7: "R2",
    8: "SELECT", 9: "START", 10: "L3", 11: "R3", 12: "UP", 13: "DOWN", 14: "LEFT", 15: "RIGHT",
  },
};

const STORAGE_KEY = "sovemu.bindings";
const STICK_THRESHOLD = 0.5;
const FAST_FORWARD_KEY = "Tab";
const REWIND_KEY = "Backspace";
const PAD_SELECT = 8;
const PAD_L = 4;
const PAD_R = 5;

export function loadBindings(storage: Pick<Storage, "getItem"> = localStorage): Bindings {
  try {
    const saved = JSON.parse(storage.getItem(STORAGE_KEY) ?? "null");
    if (saved?.keys && saved?.pad) return saved as Bindings;
  } catch { /* corrupt: use defaults */ }
  return structuredClone(DEFAULT_BINDINGS);
}

export function saveBindings(b: Bindings, storage: Pick<Storage, "setItem"> = localStorage) {
  storage.setItem(STORAGE_KEY, JSON.stringify(b));
}

/** Binds `input` to `button`, removing any other binding of that input. */
export function bind(b: Bindings, input: { key: string } | { pad: number }, button: ButtonName): Bindings {
  const next = structuredClone(b);
  if ("key" in input) next.keys[input.key] = button;
  else next.pad[input.pad] = button;
  return next;
}

/** What is bound to `button`, for display. */
export function describeBinding(b: Bindings, button: ButtonName): string {
  const keys = Object.entries(b.keys).filter(([, v]) => v === button).map(([k]) => k.replace(/^Key/, "").replace(/^Arrow/, "→ "));
  const pads = Object.entries(b.pad).filter(([, v]) => v === button).map(([i]) => `pad ${i}`);
  return [...keys, ...pads].join(", ") || "unbound";
}

export interface InputReading {
  buttons: number;
  hotkeys: Hotkeys;
}

export class InputState {
  bindings: Bindings = loadBindings();
  private held = new Set<string>();

  constructor(target: Window = window) {
    target.addEventListener("keydown", (e) => this.onKey(e, true));
    target.addEventListener("keyup", (e) => this.onKey(e, false));
    target.addEventListener("blur", () => this.held.clear());
  }

  private onKey(e: KeyboardEvent, down: boolean) {
    const relevant = e.code in this.bindings.keys || e.code === FAST_FORWARD_KEY || e.code === REWIND_KEY;
    if (!relevant) return;
    if (down) this.held.add(e.code);
    else this.held.delete(e.code);
    e.preventDefault();
  }

  /** The current state: keyboard and every connected gamepad, combined. */
  read(): InputReading {
    let buttons = 0;
    for (const code of this.held) {
      const name = this.bindings.keys[code];
      if (name) buttons |= bit(name);
    }
    const hotkeys: Hotkeys = { fastForward: this.held.has(FAST_FORWARD_KEY), rewind: this.held.has(REWIND_KEY) };

    const pads = navigator.getGamepads ? navigator.getGamepads() : [];
    for (const pad of pads) {
      if (!pad) continue;
      const pressed = (i: number) => !!pad.buttons[i]?.pressed;
      const comboFF = pressed(PAD_SELECT) && pressed(PAD_R);
      const comboRewind = pressed(PAD_SELECT) && pressed(PAD_L);
      hotkeys.fastForward ||= comboFF;
      hotkeys.rewind ||= comboRewind;
      const masked = comboFF || comboRewind ? new Set([PAD_SELECT, PAD_L, PAD_R]) : new Set<number>();
      pad.buttons.forEach((b, i) => {
        const name = this.bindings.pad[i];
        if (b.pressed && name && !masked.has(i)) buttons |= bit(name);
      });
      const [x = 0, y = 0] = pad.axes;
      if (x < -STICK_THRESHOLD) buttons |= bit("LEFT");
      if (x > STICK_THRESHOLD) buttons |= bit("RIGHT");
      if (y < -STICK_THRESHOLD) buttons |= bit("UP");
      if (y > STICK_THRESHOLD) buttons |= bit("DOWN");
    }
    return { buttons, hotkeys };
  }

  setBindings(b: Bindings) {
    this.bindings = b;
    saveBindings(b);
  }
}

/** Resolves with the next key or gamepad button pressed (for remapping). Escape cancels. */
export function captureNextInput(): Promise<{ key: string } | { pad: number } | null> {
  return new Promise((resolve) => {
    const before = new Set<string>();
    for (const pad of navigator.getGamepads?.() ?? []) {
      pad?.buttons.forEach((b, i) => b.pressed && before.add(`${pad.index}:${i}`));
    }
    let raf = 0;
    const done = (result: { key: string } | { pad: number } | null) => {
      cancelAnimationFrame(raf);
      removeEventListener("keydown", onKey, true);
      resolve(result);
    };
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopImmediatePropagation();
      done(e.code === "Escape" ? null : { key: e.code });
    };
    addEventListener("keydown", onKey, true);
    const poll = () => {
      for (const pad of navigator.getGamepads?.() ?? []) {
        if (!pad) continue;
        for (let i = 0; i < pad.buttons.length; i++) {
          const id = `${pad.index}:${i}`;
          if (pad.buttons[i].pressed && !before.has(id)) return done({ pad: i });
          if (!pad.buttons[i].pressed) before.delete(id);
        }
      }
      raf = requestAnimationFrame(poll);
    };
    raf = requestAnimationFrame(poll);
  });
}
