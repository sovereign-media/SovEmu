# SovEmu

Self-hosted console emulation streamed over WebRTC. A Rust server runs
[libretro](https://www.libretro.com/) cores and streams video, audio and input to web, Android
and iOS clients.

> **Status:** Phase 3. Sign in, pick a system and a game, play in the browser over WebRTC, save
> and continue later. See [docs/ROADMAP.md](docs/ROADMAP.md).

## Repository layout

| Path | What |
|---|---|
| `SovEmu.Backend/` | The Rust workspace: libretro bindings, the frontend, the worker, the test core |
| `docs/` | [Architecture](docs/ARCHITECTURE.md) and [roadmap](docs/ROADMAP.md) |
| `scripts/` | `fetch-cores.sh` downloads libretro cores for local testing |
| `SovEmu.Web/` | The web client: Astro + Tailwind, served by the Rust server |
| `SovEmu.Android/`, `SovEmu.iOS/` | Native clients, as submodules (`git submodule update --init --remote`) |

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

## A library on a NAS

`compose.nas.yaml` mounts an SMB share **read-only** as the library. The share should hold a
Batocera-style layout: `roms/<system>/` with `images/` and `videos/` artwork beside the ROMs,
plus `bios/`. Put the share and credentials in `.env`, which git ignores (see `.env.example`),
then:

```bash
docker compose -f compose.yaml -f compose.nas.yaml up -d
```

The scanner picks up box art, composite images, marquees and video snaps
(`images/<rom>-thumb.png`, `-image.png`, `-marquee.png`, `videos/<rom>-video.mp4`). The game list
shows them. It scans in the background, so the server answers at once. A 3,500-game SMB library
takes about 20 s.

Supported systems: NES, SNES, Game Boy, Game Boy Color, GBA, Mega Drive, Master System, Game
Gear, PlayStation, and arcade / Neo Geo (FBNeo). N64, Dreamcast, GameCube and PS2 need GPU cores,
which SovEmu doesn't run yet, so they don't appear.

## Configuration

All settings are flags on `sovemu-server` or environment variables (the image sets the paths).

| Variable | Default | What |
|---|---|---|
| `SOVEMU_HTTP` | `0.0.0.0:8080` | Web client and API address |
| `SOVEMU_DATA_DIR` | `data` (`/data` in the image) | Database, saves, `system/` (BIOS) |
| `SOVEMU_ROMS_DIR` | `<data dir>/roms` | The ROM library, one folder per system |
| `SOVEMU_SYSTEM_DIR` | `<data dir>/system` | BIOS files for cores (PlayStation, Neo Geo, …) |
| `SOVEMU_UDP_PORTS` | `50000-50009` | One UDP port per concurrent game session; publish the whole range |
| `SOVEMU_ADVERTISE_IP` | detected (`127.0.0.1` in compose) | The address browsers send media to: the host's LAN address in a container |
| `SOVEMU_ICE_SERVERS` | `[]` | STUN/TURN for browsers that cannot reach the UDP ports directly, as JSON `RTCIceServer`s |
| `SOVEMU_IDLE_TIMEOUT` | `300` | Seconds a session survives with no client connected |
| `SOVEMU_SECURE_COOKIES` | off | Set when serving over HTTPS (behind a reverse proxy) |
| `SOVEMU_METRICS_TOKEN` | unset | Enables `GET /metrics` (Prometheus) for this bearer token |

`GET /health` answers `ok` while the database is reachable. Sign-in locks a username out for five
minutes after five failed attempts.

**Playing away from home:** sessions need UDP from the browser to the session ports. Expose
8080 through an HTTPS reverse proxy (and set `SOVEMU_SECURE_COOKIES=1`). Then forward
50000-50009/udp and set `SOVEMU_ADVERTISE_IP` to your public address, or run a TURN server
(coturn) and list it in `SOVEMU_ICE_SERVERS`.

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
