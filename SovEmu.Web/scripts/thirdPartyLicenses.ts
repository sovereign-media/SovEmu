// The web client's third-party notices: a Vite plugin (after Sovereign.React's) that writes
// `third-party-licenses.json` beside `index.html`, listing every npm package whose code is in
// the production bundle, grouped by licence text, in the shape the server's own list has
// (`SovEmu.Server/src/notices.rs`). The server hands it to the admin About page over GraphQL.
//
// MIT, BSD and Apache-2.0 ask for their notice to travel with the code, and the minifier strips
// every `@license` comment, so this file is how it does. It reads the chunks the bundler wrote,
// so a tree-shaken dependency, a devDependency and a build tool that ships nothing are absent,
// and it fails the build on a licence outside ALLOWED_LICENSES.

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import type { Plugin } from "vite";

/**
 * The licences a package may carry and still ship in the bundle: each permissive, asking only
 * that its notice travel with the code. Anything else fails the build, so a copyleft or
 * unlicensed dependency is a decision someone makes, not one a lockfile bump makes for them.
 */
export const ALLOWED_LICENSES = new Set([
  "0BSD",
  "Apache-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "CC0-1.0",
  "ISC",
  "MIT",
]);

export const THIRD_PARTY_LICENSES_FILE = "third-party-licenses.json";

const LICENSE_FILE = /^(?:licen[cs]e|copying)(?:[.-].*)?$/i;
const NOTICE_FILE = /^notice(?:[.-].*)?$/i;

/** One package in the bundle. */
export interface BundledPackage {
  name: string;
  version: string;
  license: string;
  repository: string | null;
  /** Its licence file, and its NOTICE file after it, if it ships them. */
  text: string;
}

/** One licence text and the packages it covers, as the server's list has them. */
export interface Notice {
  id: string;
  name: string;
  crates: { name: string; version: string; repository: string | null }[];
  text: string;
}

interface Manifest {
  name?: string;
  version?: string;
  license?: string | { type?: string };
  repository?: string | { url?: string };
  homepage?: string;
}

/**
 * The package a bundled module belongs to: the directory under the last `node_modules/`, two
 * segments deep for a scoped name, whose `package.json` names the licence.
 */
export function packageRoot(moduleId: string): string | null {
  const id = moduleId.replace(/\\/g, "/").split("?")[0];
  const marker = id.lastIndexOf("/node_modules/");
  if (marker < 0) return null;
  const segments = id.slice(marker + "/node_modules/".length).split("/");
  const depth = segments[0].startsWith("@") ? 2 : 1;
  return `${id.slice(0, marker)}/node_modules/${segments.slice(0, depth).join("/")}`;
}

/** Whether an SPDX expression allows the package: any one of its choices (`MIT OR Apache-2.0`). */
export function allowed(license: string): boolean {
  return license.split(/\s+(?:OR|AND)\s+|[()]/).some((id) => ALLOWED_LICENSES.has(id.trim()));
}

/** Packages grouped by licence and text, each group's packages and the groups in name order. */
export function group(packages: BundledPackage[]): Notice[] {
  const groups = new Map<string, Notice>();
  for (const p of packages) {
    const key = `${p.license}\n${p.text}`;
    const g = groups.get(key) ?? { id: p.license, name: p.license, crates: [], text: p.text };
    g.crates.push({ name: p.name, version: p.version, repository: p.repository });
    groups.set(key, g);
  }
  const byName = (a: { name: string }, b: { name: string }) => a.name.localeCompare(b.name);
  return [...groups.values()]
    .map((g) => ({ ...g, crates: g.crates.sort(byName) }))
    .sort((a, b) => a.id.localeCompare(b.id) || byName(a.crates[0], b.crates[0]));
}

function readFirst(directory: string, pattern: RegExp): string | null {
  const name = readdirSync(directory)
    .filter((entry) => pattern.test(entry))
    .sort()[0];
  return name ? readFileSync(join(directory, name), "utf8") : null;
}

function repository(m: Manifest): string | null {
  const url = typeof m.repository === "string" ? m.repository : m.repository?.url;
  return (url ?? m.homepage ?? null)?.replace(/^git\+/, "").replace(/\.git$/, "") ?? null;
}

function describe(root: string): BundledPackage {
  const m = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as Manifest;
  const license = typeof m.license === "string" ? m.license : (m.license?.type ?? "");
  const text = [readFirst(root, LICENSE_FILE), readFirst(root, NOTICE_FILE)].filter(Boolean).join("\n\n");
  return { name: m.name ?? root, version: m.version ?? "", license, repository: repository(m), text: text || license };
}

export function thirdPartyLicenses(): Plugin {
  return {
    name: "sovemu:third-party-licenses",
    apply: "build",
    generateBundle(_options, bundle) {
      const roots = new Set<string>();
      for (const output of Object.values(bundle)) {
        if (output.type !== "chunk") continue;
        for (const id of output.moduleIds) {
          const root = packageRoot(id);
          if (root && existsSync(join(root, "package.json"))) roots.add(root);
        }
      }
      const packages = new Map<string, BundledPackage>();
      for (const root of roots) {
        const p = describe(root);
        packages.set(`${p.name}@${p.version}`, p);
      }
      const rejected = [...packages.values()].filter((p) => !allowed(p.license));
      if (rejected.length > 0) {
        this.error(
          `Bundled packages with a licence outside the allowed set (scripts/thirdPartyLicenses.ts): ${rejected
            .map((p) => `${p.name}@${p.version} (${p.license || "none"})`)
            .join(", ")}`,
        );
      }
      this.emitFile({
        type: "asset",
        fileName: THIRD_PARTY_LICENSES_FILE,
        source: JSON.stringify(group([...packages.values()])),
      });
    },
  };
}
