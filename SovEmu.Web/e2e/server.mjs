// Starts a real sovemu-server for the end-to-end tests (Playwright's `webServer`): the web client
// built into dist/, the API on SOVEMU_E2E_PORT (18080), a fresh data folder each run
// (test-results/e2e-data, wiped as a run starts and left for a look afterwards), a small
// library of stand-in games, and an admin from the environment. Builds what it runs first
// (cargo debug build of the server and worker, `npm run build`), unless SOVEMU_E2E_SKIP_BUILD=1.
//
// The stand-in games are a few bytes named like ROMs: the scanner places files by folder and
// extension, so they list like games but are not ones (never commit a real ROM).

import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const web = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const backend = resolve(web, "../SovEmu.Backend");
const port = process.env.SOVEMU_E2E_PORT ?? "18080";

if (process.env.SOVEMU_E2E_SKIP_BUILD !== "1") {
  execFileSync("cargo", ["build", "-p", "sovemu-server", "-p", "sovemu-worker"], { cwd: backend, stdio: "inherit" });
  execFileSync("npm", ["run", "build"], { cwd: web, stdio: "inherit" });
}
const server = join(backend, "target/debug/sovemu-server");
if (!existsSync(server)) throw new Error(`${server} is missing: build it, or unset SOVEMU_E2E_SKIP_BUILD`);

const data = join(web, "test-results/e2e-data");
rmSync(data, { recursive: true, force: true });
const games = {
  // A file no system plays: "Needs attention" lists it under Not games.
  nes: ["Stand-in Quest (USA).nes", "Another Stand-in (Europe).nes", "stand-in notes.abc"],
  // A soft patch named like its game: its default (the Patches page lists it).
  snes: ["Super Stand-in (USA).sfc", "Super Stand-in (USA).ips"],
  gb: ["Pocket Stand-in (Japan).gb"],
  // Systems that read a BIOS, for the BIOS table.
  psx: ["Disc Stand-in (USA).chd"],
  lynx: ["Handheld Stand-in (USA).lnx"],
};
for (const [system, files] of Object.entries(games)) {
  mkdirSync(join(data, "roms", system), { recursive: true });
  for (const file of files) writeFileSync(join(data, "roms", system, file), "stand-in");
}

const child = spawn(
  server,
  ["--data-dir", data, "--roms-dir", join(data, "roms"), "--web-dir", join(web, "dist"), "--http", `127.0.0.1:${port}`],
  {
    stdio: "inherit",
    env: {
      ...process.env,
      SovEmu__Authentication__AdminUsername: process.env.SOVEMU_E2E_USERNAME ?? "admin",
      SovEmu__Authentication__AdminPassword: process.env.SOVEMU_E2E_PASSWORD ?? "e2e-password",
      // No lookups against outside services from a test run.
      SovEmu__Metadata__AutoLookup: "false",
      RUST_LOG: process.env.RUST_LOG ?? "warn",
    },
  },
);

// Playwright stops the run by ending this process and the server with it; pass on a signal sent
// to this process alone, so the server still shuts down properly.
const stop = () => child.kill("SIGTERM");
process.on("SIGTERM", stop);
process.on("SIGINT", stop);
child.on("exit", (code) => process.exit(code ?? 0));
