import { describe, expect, it, vi } from "vitest";
import { type MenuItem, MenuStack } from "./menu";

const item = (label: string, extra: Partial<MenuItem> = {}): MenuItem => ({ label, run: vi.fn(), ...extra });

describe("MenuStack", () => {
  it("moves, wraps and skips disabled items", () => {
    const changes = vi.fn();
    const m = new MenuStack(changes);
    m.push({ title: "t", items: [item("a", { disabled: true }), item("b"), item("c")] });
    expect(m.current?.index).toBe(1);
    m.handle("down");
    expect(m.current?.index).toBe(2);
    m.handle("down");
    expect(m.current?.index).toBe(1);
    m.handle("up");
    expect(m.current?.index).toBe(2);
    expect(changes).toHaveBeenCalled();
  });

  it("runs the selected item and pops on back", () => {
    const m = new MenuStack(() => {});
    const run = vi.fn();
    m.push({ title: "main", items: [item("x", { run })] });
    m.push({ title: "sub", items: [item("y")] });
    m.handle("back");
    expect(m.current?.menu.title).toBe("main");
    m.handle("accept");
    expect(run).toHaveBeenCalledOnce();
    m.handle("back");
    expect(m.open).toBe(false);
  });

  it("uses a custom back handler", () => {
    const m = new MenuStack(() => {});
    const onBack = vi.fn();
    m.push({ title: "splash", items: [item("x")], onBack });
    m.handle("back");
    expect(onBack).toHaveBeenCalledOnce();
    expect(m.open).toBe(true);
  });
});
