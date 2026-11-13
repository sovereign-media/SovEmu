# SovEmu

Self-hosted console emulation streamed over WebRTC. A Rust server runs
[libretro](https://www.libretro.com/) cores and streams video, audio and input to web, Android
and iOS clients.

> **Status:** Phase 1. One game streams to a browser over WebRTC (VP8 + Opus, input over a
> DataChannel). There's no library, accounts or persistence yet. See [docs/ROADMAP.md](docs/ROADMAP.md).

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
docker compose build
docker compose run --rm sovemu probe                  # list the bundled cores
cp game.nes data/roms/
SOVEMU_ROM=roms/game.nes docker compose up            # then open http://localhost:8080
```

`./data` is mounted at `/data`. Put ROMs in `data/roms/` and BIOS files in `data/system/`.
Pick another core with `SOVEMU_CORE=/opt/sovemu/cores/snes9x_libretro.so`. Browsers on other
machines need `SOVEMU_ADVERTISE_IP` set to this host's LAN address (it defaults to `127.0.0.1`),
plus UDP 50000 reachable.

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
target/release/sovemu-worker serve --core <core> --rom <game> --web-dir ../SovEmu.Web/dist
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
