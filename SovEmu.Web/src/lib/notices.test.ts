import { describe, expect, it } from "vitest";
import type { Notice } from "./api";
import { byLicence, packagesText } from "./notices";

const notice = (id: string, ...names: string[]): Notice => ({
  id,
  text: `${id} text`,
  packages: names.map((name) => ({ name, version: "1.0.0", repository: null })),
});

describe("notices", () => {
  it("groups texts by licence, the most used first", () => {
    const groups = byLicence([notice("MIT", "a"), notice("Apache-2.0", "b", "c"), notice("MIT", "d")]);
    expect(groups.map((g) => [g.id, g.packages, g.texts.length])).toEqual([
      ["Apache-2.0", 2, 1],
      ["MIT", 2, 2],
    ]);
  });

  it("lists a text's packages with their versions", () => {
    expect(packagesText(notice("MIT", "axum", "tokio"))).toBe("axum 1.0.0, tokio 1.0.0");
  });
});
