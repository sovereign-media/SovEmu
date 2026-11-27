// A small stack of menus, driven by navigation actions: the player's splash, its in-game menu
// and the save-slot pickers all use it. Rendering is the caller's; this keeps the state.
import type { PadLayout } from "./api";
import type { NavAction } from "./nav";

export interface MenuItem {
  label: string;
  /** Secondary text (a save's time, say). */
  detail?: string;
  /** A Lucide icon to show beside the label, by name (the page maps names to icons). */
  icon?: string;
  /** An image to show beside the label. */
  image?: string;
  disabled?: boolean;
  /** Keys to light on the menu's diagrams while this item is selected. */
  highlight?: { system?: string[]; controller?: string[] };
  /** A rule above this item, setting it apart (Quit game, as the Android app does). */
  divider?: boolean;
  run: () => void | Promise<void>;
}

export interface Menu {
  title: string;
  /** Head the menu with the game's system badge, as the Android app's game menu does. */
  badge?: boolean;
  items: MenuItem[];
  /** What Back does at this level; the default pops the menu. */
  onBack?: () => void;
  /** Pads drawn beside the items: the console's and the player's controller's. */
  diagrams?: { system?: PadLayout | null; controller?: PadLayout | null };
}

export class MenuStack {
  private stack: { menu: Menu; index: number }[] = [];

  constructor(private readonly onChange: () => void) {}

  get current(): { menu: Menu; index: number } | undefined {
    return this.stack[this.stack.length - 1];
  }

  get open(): boolean {
    return this.stack.length > 0;
  }

  push(menu: Menu) {
    const first = menu.items.findIndex((i) => !i.disabled);
    this.stack.push({ menu, index: Math.max(0, first) });
    this.onChange();
  }

  /** Replaces the top menu (to refresh it in place), keeping the selection where possible. */
  replace(menu: Menu) {
    const top = this.stack.pop();
    this.stack.push({ menu, index: Math.min(top?.index ?? 0, Math.max(0, menu.items.length - 1)) });
    this.onChange();
  }

  pop() {
    this.stack.pop();
    this.onChange();
  }

  close() {
    this.stack = [];
    this.onChange();
  }

  /** Moves to an item by index (a pointer click), then runs it. */
  activate(index: number) {
    const top = this.current;
    if (!top || top.menu.items[index]?.disabled) return;
    top.index = index;
    void top.menu.items[index].run();
  }

  handle(action: NavAction) {
    const top = this.current;
    if (!top) return;
    const { items } = top.menu;
    const step = (dir: number) => {
      for (let n = 1; n <= items.length; n++) {
        const i = (top.index + dir * n + items.length * n) % items.length;
        if (!items[i].disabled) return i;
      }
      return top.index;
    };
    if (action === "up") top.index = step(-1);
    else if (action === "down") top.index = step(1);
    else if (action === "accept") return void items[top.index]?.run();
    else if (action === "back") return top.menu.onBack ? top.menu.onBack() : this.pop();
    else return;
    this.onChange();
  }
}
