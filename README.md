# SovEmu

Self-hosted console emulation streamed over WebRTC. A Rust server runs
[libretro](https://www.libretro.com/) cores and streams video, audio and input to web, Android
and iOS clients.

> **Status:** Phase 0. The libretro frontend runs real cores headless. No streaming yet. See
> [docs/ROADMAP.md](docs/ROADMAP.md).

## Repository layout

| Path | What |
|---|---|
| `SovEmu.Backend/` | The Rust workspace: libretro bindings, the frontend, the worker, the test core |
| `docs/` | [Architecture](docs/ARCHITECTURE.md) and [roadmap](docs/ROADMAP.md) |
| `scripts/` | `fetch-cores.sh` downloads libretro cores for local testing |
| `SovEmu.Web/`, `SovEmu.Android/`, `SovEmu.iOS/` | Clients, as submodules (planned) |

## Container

SovEmu ships as one self-contained Alpine Linux image. The libretro cores are built from
source at pinned commits ([scripts/cores.list](scripts/cores.list)), and every core is checked
for loading during the build.

```bash
docker compose build
docker compose run --rm sovemu probe          # list the bundled cores
docker compose run --rm sovemu headless \
  --core /opt/sovemu/cores/fceumm_libretro.so --rom roms/game.nes --frames 600 --out headless-out
```

`./data` is mounted at `/data`. Put ROMs in `data/roms/` and BIOS files in `data/system/`.
Output lands in `data/headless-out/`.

## Building from source

Needs Rust (stable, 1.85+) and libclang (for bindgen): Xcode Command Line Tools on macOS,
`libclang-dev` on Debian/Ubuntu.

```bash
cd SovEmu.Backend
cargo test --workspace
cargo build --release -p sovemu-worker
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
