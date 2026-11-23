import { describe, expect, it } from "vitest";
import { allowed, group, packageRoot } from "./thirdPartyLicenses";

describe("third-party licences", () => {
  it("finds the package a module belongs to", () => {
    expect(packageRoot("/w/node_modules/react/cjs/react.production.js")).toBe("/w/node_modules/react");
    expect(packageRoot("/w/node_modules/@tanstack/react-query/build/index.js")).toBe(
      "/w/node_modules/@tanstack/react-query",
    );
    expect(packageRoot("/w/node_modules/a/node_modules/b/x.js?v=1")).toBe("/w/node_modules/a/node_modules/b");
    expect(packageRoot("/w/src/main.tsx")).toBeNull();
  });

  it("allows a package when any choice of its expression is allowed", () => {
    expect(allowed("MIT")).toBe(true);
    expect(allowed("(MIT OR GPL-3.0)")).toBe(true);
    expect(allowed("GPL-3.0")).toBe(false);
    expect(allowed("")).toBe(false);
  });

  it("groups packages by licence and text, in name order", () => {
    const mit = (name: string, text = "MIT text") => ({
      name,
      version: "1.0.0",
      license: "MIT",
      repository: null,
      text,
    });
    const notices = group([mit("b"), mit("a"), mit("c", "other text")]);
    expect(notices.map((n) => n.crates.map((c) => c.name))).toEqual([["a", "b"], ["c"]]);
    expect(notices[0]).toMatchObject({ id: "MIT", text: "MIT text" });
  });
});
