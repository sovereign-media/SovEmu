// Menu navigation from keyboard and gamepads, as abstract actions.
//
// Directions auto-repeat while held (after a delay, then faster), like a console menu; buttons
// fire once per press. Gamepads are polled every animation frame (the Gamepad API has no
// events for buttons), keyboards via events.

export type NavAction =
  | "up"
  | "down"
  | "left"
  | "right"
  | "accept"
  | "back"
  | "menu"
  | "pageUp"
  | "pageDown"
  | "search";

const KEY_ACTIONS: Record<string, NavAction> = {
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
  Enter: "accept",
  KeyX: "accept",
  " ": "accept",
  Space: "accept",
  Escape: "back",
  Backspace: "back",
  KeyZ: "back",
  Tab: "menu",
  PageUp: "pageUp",
  PageDown: "pageDown",
  Slash: "search",
};

// W3C standard gamepad: 0 = bottom face (RetroPad B), 1 = right face (RetroPad A).
// Console menus confirm with the right face button and go back with the bottom one; the top one
// (RetroPad X) searches.
const PAD_ACTIONS: Record<number, NavAction> = {
  1: "accept",
  0: "back",
  3: "search",
  9: "menu",
  4: "pageUp",
  5: "pageDown",
  12: "up",
  13: "down",
  14: "left",
  15: "right",
};

const REPEATABLE = new Set<NavAction>(["up", "down", "left", "right", "pageUp", "pageDown"]);
export const REPEAT_DELAY_MS = 350;
export const REPEAT_INTERVAL_MS = 90;

/** Decides when a held direction fires again. Pure, so it can be tested. */
export class Repeater {
  private since = new Map<NavAction, { start: number; last: number }>();

  /** Feeds the set of held actions at time `now`; returns the actions that fire. */
  update(held: Set<NavAction>, now: number): NavAction[] {
    const fired: NavAction[] = [];
    for (const action of held) {
      const state = this.since.get(action);
      if (!state) {
        this.since.set(action, { start: now, last: now });
        fired.push(action);
      } else if (
        REPEATABLE.has(action) &&
        now - state.start >= REPEAT_DELAY_MS &&
        now - state.last >= REPEAT_INTERVAL_MS
      ) {
        state.last = now;
        fired.push(action);
      }
    }
    for (const action of [...this.since.keys()]) if (!held.has(action)) this.since.delete(action);
    return fired;
  }
}

function padHeld(): Set<NavAction> {
  const held = new Set<NavAction>();
  for (const pad of navigator.getGamepads ? navigator.getGamepads() : []) {
    if (!pad) continue;
    pad.buttons.forEach((b, i) => {
      const action = PAD_ACTIONS[i];
      if (b.pressed && action) held.add(action);
    });
    const [x = 0, y = 0] = pad.axes;
    if (x < -0.5) held.add("left");
    if (x > 0.5) held.add("right");
    if (y < -0.5) held.add("up");
    if (y > 0.5) held.add("down");
  }
  return held;
}

/** Calls `handler` for every navigation action until `stop()` is called. */
export function listenNav(handler: (action: NavAction) => void): { stop: () => void; paused: boolean } {
  const keys = new Set<NavAction>();
  const repeater = new Repeater();
  const control = { stop: () => {}, paused: false };

  const isTyping = (e: KeyboardEvent) =>
    e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
  // Keys pressed inside an open menu (the side nav's) belong to the menu, not to the page.
  const inMenu = (e: KeyboardEvent) =>
    e.target instanceof Element && !!e.target.closest('[role="menu"], [role="menubar"]');
  const down = (e: KeyboardEvent) => {
    const action = KEY_ACTIONS[e.code] ?? KEY_ACTIONS[e.key];
    if (!action || isTyping(e) || inMenu(e) || control.paused) return;
    keys.add(action);
    e.preventDefault();
  };
  const up = (e: KeyboardEvent) => {
    const action = KEY_ACTIONS[e.code] ?? KEY_ACTIONS[e.key];
    if (action) keys.delete(action);
  };
  addEventListener("keydown", down);
  addEventListener("keyup", up);
  const blur = () => keys.clear();
  addEventListener("blur", blur);

  let raf = 0;
  const tick = (now: number) => {
    if (!control.paused) {
      const held = new Set([...keys, ...padHeld()]);
      for (const action of repeater.update(held, now)) handler(action);
    }
    raf = requestAnimationFrame(tick);
  };
  raf = requestAnimationFrame(tick);

  control.stop = () => {
    cancelAnimationFrame(raf);
    removeEventListener("keydown", down);
    removeEventListener("keyup", up);
    removeEventListener("blur", blur);
  };
  return control;
}
