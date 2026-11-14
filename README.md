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
plus `bios/`. Put the share, credentials and mode in `.env`, which git ignores (see
`.env.example`), then:

```bash
docker compose -f compose.yaml -f compose.nas.yaml up -d
```

Choose how the server uses the share with `SOVEMU_LIBRARY_MODE`:

| Mode | When | What happens |
|---|---|---|
| `direct` (default) | The NAS is on the same network | Games and artwork are read straight from the share |
| `cache` | The NAS is remote (VPN, a Cloudflare tunnel) | Played games and seen artwork are kept in `data/cache` (`SOVEMU_CACHE_GB`, least recently used out). Small games are copied on first play; disc images are copied after their first session. Opening a system prefetches its box art. A 32 MB GBA game: 7.9 s cold, 0.3 s warm over a tunnel. |
| `mirror` | You want a full local copy | A background sync copies the library and its artwork into `data/library`, copying only what changed and removing what was deleted. Games are played from the copy. `SOVEMU_MIRROR_SYSTEMS=nes,snes` limits it. Start a sync from the Library page. |

The scanner picks up box art, composite images, marquees and video snaps
(`images/<rom>-thumb.png`, `-image.png`, `-marquee.png`, `videos/<rom>-video.mp4`). The game list
shows them. Scans run in the background, in parallel.

## Game information

The **Library** page (admins) fetches titles, descriptions, release dates, developers,
publishers, genres, player counts and box art. Missing box art is downloaded. Sources, in order:

1. **FinalBurn Neo's game lists**, for arcade and Neo Geo sets. No account needed:
   `mslug` becomes *Metal Slug - Super Vehicle-001* (1996, Nazca).
2. **[ScreenScraper](https://www.screenscraper.fr)**, the database Recalbox, Batocera, RetroBat and
   ES-DE scrape from. It matches by MD5, size and file name. It needs developer credentials
   (requested on its forum); a free user account raises the daily quota.
3. **[TheGamesDB](https://thegamesdb.net)**, by name and platform. It needs an API key (requested
   on its forum).

Add credentials in `.env` (`SOVEMU_SCREENSCRAPER_*`, `SOVEMU_THEGAMESDB_KEY`). A job stops
using a service that rejects credentials or runs out of quota, and retries those games next time.

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
| `SOVEMU_LIBRARY_MODE` | `direct` | `direct`, `cache` or `mirror` (see above) |
| `SOVEMU_CACHE_GB` | `20` | `cache` mode: GiB of games and artwork to keep locally |
| `SOVEMU_MIRROR_SYSTEMS` | all | `mirror` mode: comma-separated system ids to copy |
| `SOVEMU_SCREENSCRAPER_DEVID`, `_DEVPASSWORD`, `_USER`, `_PASSWORD` | unset | ScreenScraper credentials |
| `SOVEMU_THEGAMESDB_KEY` | unset | TheGamesDB API key |
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
