// Keyboard and gamepad state, as a RetroPad bitmask for port 0.
import { bit, type ButtonName } from "./protocol";

const KEYS: Record<string, ButtonName> = {
  ArrowUp: "UP", ArrowDown: "DOWN", ArrowLeft: "LEFT", ArrowRight: "RIGHT",
  KeyZ: "B", KeyX: "A", KeyA: "Y", KeyS: "X", KeyQ: "L", KeyW: "R",
  Enter: "START", ShiftLeft: "SELECT", ShiftRight: "SELECT",
};

// W3C "standard" gamepad layout -> RetroPad, by position: bottom face = B, right face = A,
// left face = Y, top face = X (the SNES layout libretro's RetroPad is modelled on).
const PAD: Record<number, ButtonName> = {
  0: "B", 1: "A", 2: "Y", 3: "X", 4: "L", 5: "R", 6: "L2", 7: "R2",
  8: "SELECT", 9: "START", 10: "L3", 11: "R3", 12: "UP", 13: "DOWN", 14: "LEFT", 15: "RIGHT",
};

const STICK_THRESHOLD = 0.5;

export class InputState {
  private keyboard = 0;

  constructor(target: Window = window) {
    target.addEventListener("keydown", (e) => this.onKey(e, true));
    target.addEventListener("keyup", (e) => this.onKey(e, false));
    target.addEventListener("blur", () => (this.keyboard = 0));
  }

  private onKey(e: KeyboardEvent, down: boolean) {
    const name = KEYS[e.code];
    if (!name) return;
    if (down) this.keyboard |= bit(name);
    else this.keyboard &= ~bit(name);
    e.preventDefault();
  }

  /** The current state: keyboard and every connected gamepad, combined. */
  read(): number {
    let state = this.keyboard;
    const pads = navigator.getGamepads ? navigator.getGamepads() : [];
    for (const pad of pads) {
      if (!pad) continue;
      pad.buttons.forEach((b, i) => {
        const name = PAD[i];
        if (b.pressed && name) state |= bit(name);
      });
      const [x = 0, y = 0] = pad.axes;
      if (x < -STICK_THRESHOLD) state |= bit("LEFT");
      if (x > STICK_THRESHOLD) state |= bit("RIGHT");
      if (y < -STICK_THRESHOLD) state |= bit("UP");
      if (y > STICK_THRESHOLD) state |= bit("DOWN");
    }
    return state;
  }
}
