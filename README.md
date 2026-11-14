# SovEmu

Self-hosted console emulation streamed over WebRTC. A Rust server runs
[libretro](https://www.libretro.com/) cores and streams video, audio and input to web, Android
and iOS clients.

> **Status:** Phase 2. Sign in, pick a system and a game, and play in the browser over WebRTC.
> Saving games comes in Phase 3. See [docs/ROADMAP.md](docs/ROADMAP.md).

## Repository layout

| Path | What |
|---|---|
| `SovEmu.Backend/` | The Rust workspace: libretro bindings, the frontend, the worker, the test core |
| `docs/` | [Architecture](docs/ARCHITECTURE.md) and [roadmap](docs/ROADMAP.md) |
| `scripts/` | `fetch-cores.sh` downloads libretro cores for local testing |
| `SovEmu.Web/` | The web client: Astro + Tailwind, served by the Rust server |
| `SovEmu.Android/`, `SovEmu.iOS/` | Native clients, as submodules (planned) |

## Container

SovEmu ships as one self-contained Alpine Linux image. The libretro cores are built from
source at pinned commits ([scripts/cores.list](scripts/cores.list)), and every core is checked
for loading during the build.

```bash
scripts/fetch-free-roms.sh            # free homebrew into data/roms, or add your own
docker compose up --build             # then open http://localhost:8080
```

The first visit creates the admin account. `./data` is mounted at `/data`: the database,
`data/roms/` (one folder per system: `nes/`, `snes/`, `gb/`, `gbc/`, `gba/`, `megadrive/`,
`mastersystem/`, `gamegear/`), `data/system/` for BIOS files and `data/saves/`. Browsers on other
machines need `SOVEMU_ADVERTISE_IP` set to this host's LAN address (it defaults to `127.0.0.1`), plus
UDP 50000-50009 reachable.

## Building from source

You need:
- Rust (stable, 1.89+);
- libclang, for bindgen;
- libvpx, opus and pkg-config;
- Node 22.23.4+ for the web client.

On macOS: Xcode Command Line Tools and `brew install libvpx opus pkgconf`. On Debian/Ubuntu:
`libclang-dev libvpx-dev libopus-dev pkg-config`.

```bash
(cd SovEmu.Web && npm ci && npm test && npm run build)
cd SovEmu.Backend
cargo test --workspace
cargo build --release -p sovemu-worker
cargo build --release -p sovemu-server
target/release/sovemu-server --data-dir ../data --cores-dir ../cores --web-dir ../SovEmu.Web/dist
```

## Running a core headless

```bash
scripts/fetch-cores.sh fceumm
SovEmu.Backend/target/release/sovemu-worker headless \
  --core cores/fceumm_libretro.dylib \
  --rom /path/to/game.nes \
  --frames 600 --png-every 60 --hold 200-205:start \
  --out headless-out
```

This writes `frame_NNNNNN.png`, `last.png`, `audio.wav` and `summary.json` (core info, A/V
timing, per-frame core time) to `headless-out/`. `--realtime` paces at the core's frame rate;
`--var key=value` sets a core option. Run `sovemu-worker headless --help` for everything else.

Cores, ROMs and BIOS files are never committed.
