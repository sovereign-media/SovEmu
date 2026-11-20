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
| `charts/sovemu/` | The Helm chart |
| `SovEmu.Web/` | The web client: Astro + Tailwind, React with Radix UI and Lucide icons, served by the Rust server |
| `SovEmu.Android/`, `SovEmu.iOS/` | Native clients, as submodules (`git submodule update --init --remote`) |

## Container

SovEmu ships as one self-contained Debian image for x86_64, the same on every host, with or
without a GPU. The libretro cores come prebuilt from a libretro stable release (1.22.2), pinned
by version and checksum in [scripts/cores.list](scripts/cores.list), and every core is checked
for loading during the build. libretro's stable releases have no Linux arm64 cores, so the image
is x86_64 only; an arm64 machine (a Mac) builds and runs it under x86_64 emulation.

```bash
scripts/fetch-free-roms.sh            # free homebrew into data/roms, or add your own
docker compose up --build             # then open http://localhost:8080
```

The first visit creates the admin account. `./data` is mounted at `/data`: the database,
`data/roms/` (one folder per system: `nes/`, `snes/`, `gb/`, `gbc/`, `gba/`, `megadrive/`,
`mastersystem/`, `gamegear/`, `psx/`, `n64/`, `dreamcast/`, `gamecube/`, `ps2/`, …), `data/bios/`
for BIOS files (see [BIOS files](#bios-files)) and `data/saves/`. Browsers on other machines need `SOVEMU_ADVERTISE_IP` set to this host's LAN address (it defaults to `127.0.0.1`), plus
UDP 50000-50009 reachable.

## BIOS files

BIOS files live on the server, never in the library: one folder per system in `data/bios/`
(`SOVEMU_BIOS_DIR`), named like the system's folder of games. The server creates the folders.
Put each file under the name below; letter case matters. **Systems → BIOS files** in the web
client checks each one by name and MD5 against libretro's list of known dumps (`System.dat`),
finds known dumps under the wrong name or in the wrong folder, and the Systems menu marks a
system that cannot start games without one.

| Folder | Files | Without them |
|---|---|---|
| `bios/ps2/` | any PS2 BIOS dump, any name (`*.bin`) | **no games start** |
| `bios/neogeo/` | `neogeo.zip` (or beside the games in `roms/neogeo/`) | **no games start** |
| `bios/psx/` | one of `scph5501.bin`, `scph5500.bin`, `scph5502.bin`, `scph1001.bin`, `scph7001.bin`, `scph101.bin` | built-in BIOS: most games play |
| `bios/dreamcast/` | `dc_boot.bin`; optionally `dc_flash.bin` | built-in BIOS: many games play |
| `bios/gba/` | `gba_bios.bin` | built-in BIOS: almost every game plays |
| `bios/fbneo/` | `neogeo.zip`, for Neo Geo games in the arcade folder | those games only |
| `bios/nes/` | `disksys.rom` | no Famicom Disk System games |
| `bios/gb/`, `bios/gbc/` | `gb_bios.bin`, `gbc_bios.bin` | no boot logo |
| `bios/megadrive/`, `bios/mastersystem/`, `bios/gamegear/` | `bios_MD.bin`; `bios_E.sms`, `bios_U.sms`, `bios_J.sms`; `bios.gg` | no start-up screen |
| `bios/gamecube/` | `USA/IPL.bin`, `EUR/IPL.bin`, `JAP/IPL.bin` | no start-up animation |

Each core finds its files where it expects them: the worker links the system's folder into
the core's system directory (Flycast's `dc/`, PCSX2's `pcsx2/bios/`, Dolphin's
`dolphin-emu/Sys/GC/`). Files a core writes there (Flycast's `dc_nvmem.bin`) land in the
system's folder.

## 3D systems and the GPU

N64, Dreamcast, GameCube and PS2 render with OpenGL. At startup the server tries every GPU the
container can see: it makes a context on each, draws a frame and reads it back. It uses the
first one that works. Without one, it falls back to Mesa's software renderer (llvmpipe), which is
fine for many N64 games and slow for GameCube and PS2. The startup log says which:

```
GPU probe:
  /dev/dri/renderD128 (amdgpu, hardware): works: AMD Radeon RX 6600 (radeonsi, navi23, …)
  software renderer (software): works: llvmpipe (LLVM 22.1.3, 128 bits)
3D systems render on the GPU: AMD Radeon RX 6600 (radeonsi, navi23, …)
```

The API's `gpu` query (admin) returns the same, and each 3D system's `rendering` is `GPU`,
`SOFTWARE` or `OFF`.

- **Docker, AMD or Intel:** `docker compose -f compose.yaml -f compose.gpu.yaml up -d` passes
  `/dev/dri` through. Set `SOVEMU_RENDER_GID` in `.env` to the group that owns the render node
  (`stat -c %g /dev/dri/renderD128` on the host).
- **Docker, NVIDIA:** install NVIDIA's [container
  toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/) on the host, then
  `docker compose -f compose.yaml -f compose.nvidia.yaml up -d`. The toolkit adds the host's
  NVIDIA driver to the container; the image asks it for OpenGL/EGL, Vulkan and NVENC.
- **Kubernetes:** set `gpu.resources` to your GPU device plugin's resource (`amd.com/gpu: 1`,
  `gpu.intel.com/i915: 1`, `nvidia.com/gpu: 1`), or use `gpu.hostDevice`. For NVIDIA, also set
  `gpu.runtimeClassName: nvidia` if the cluster does not make NVIDIA's runtime the default.
- **Which GPUs:** AMD (radeonsi) and Intel (iris, crocus) through Mesa in the image; NVIDIA
  through NVIDIA's own driver (or Mesa's nouveau/NVK on the open-source driver).
- **Dolphin** needs its `Sys` data folder. The image ships it beside the cores and overlays it on
  the BIOS files, so neither is changed.
- **PS2 needs a BIOS dump**, and Dreamcast plays best with one: see [BIOS files](#bios-files).
- **Memory cards** for Dreamcast (VMUs) and PS2 are kept per game in the player's save folder,
  never under the shared BIOS folder.
- **GameCube needs `/dev/shm`**, at least 1 GB: Dolphin maps its emulated memory there.
  `compose.yaml` sets `shm_size` and the chart mounts one (`shmSize`). Docker's default 64 MB
  makes Dolphin die with a bus error.
- **Analog sticks:** for 3D systems, the web client sends the gamepad's sticks and triggers as
  analog values. For 2D systems the left stick still acts as a D-pad.

## Emulator settings

Each emulator has its own settings: rendering, internal resolution, frame skipping and so on
(43 for Snes9x, 113 for Dolphin). Admins change them per system on the **Settings** page: pick
the system from the dropdown, and its settings are grouped (System, Video, Audio, Input, then the
emulator's own groups), each with the emulator's explanation of what it does. A few emulators
list their settings only once a game is loaded: the worker loads a tiny stand-in game for those
(an empty NES cartridge, a GameCube program that does nothing) to read them. Arcade has no such
stand-in, so its page fills in after any arcade game has been played once. The worker speaks
libretro's core options v2, so groups, explanations and value labels come from the emulator
itself; settings an emulator leaves ungrouped are placed by keyword. Changes apply to the next
game started.

A game starts with each setting taken from these, in order, the last one winning:

1. the emulator's default;
2. SovEmu's default for the system (Dreamcast memory cards per game, for example);
3. the admin's choice;
4. the environment (`SOVEMU_SYSTEMS__<id>__OPTIONS__<name>`).

The environment always wins, and those settings show as locked on the page. The page also
lists the admin's changes as environment variables to copy into `.env` or the Helm values, so
the configuration never depends on the page. In the API: the `emulatorSettings` query and the
`changeEmulatorSettings` mutation (admin).

## Loading progress

Starting a game can take a while on a slow library, so the player shows a loading bar. It
covers each step: saving the previous game, starting the emulator, opening the core and reading
the game, with the megabytes read and the seconds taken. The bar is exact when SovEmu reads the
file. When a core reads a disc itself, the bar eases forward while the megabytes count up.

Clients give `startSession` a `startId` and poll the `startProgress(startId)` query while the
mutation waits. Clients that don't send one work as before.

## The API

SovEmu's API is GraphQL, at `POST /graphql`; open `/graphql` in a browser for GraphiQL. The web,
Android and iOS clients all use it. The schema, with every type and field documented, is
[SovEmu.Backend/SovEmu.Server/schema.graphql](SovEmu.Backend/SovEmu.Server/schema.graphql).

- **Sign-in** is a cookie: the `login` (or first-run `setup`) mutation sets it, `logout` clears it.
- **Errors** come back in `errors`, each with an `extensions.code`: `UNAUTHENTICATED`,
  `FORBIDDEN`, `NOT_FOUND`, `BAD_REQUEST`, `CONFLICT`, `GONE`, `TOO_MANY_REQUESTS`, `UNAVAILABLE`.
- **Files** (artwork, videos, save screenshots) are plain HTTP under `/media/`, at the URLs the
  API returns (`Game.mediaUrl`, `SaveSlot.screenshotUrl`).
- **Playing** a game: `startSession`, then `sessionOffer` with the browser's WebRTC offer (it
  returns the answer). Media and controller input then flow over WebRTC.

## A library on a NAS

`compose.nas.yaml` mounts an SMB share **read-only** as the library. The share should hold a
Batocera-style layout: `roms/<system>/` with `images/` and `videos/` artwork beside the ROMs.
BIOS files stay local, in `data/bios/` (copy a Batocera `bios/` folder's files into the
per-system folders). Put the share, credentials and mode in `.env`, which git ignores (see
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

### Soft patches

Translations, fixes and hacks come as IPS, UPS or BPS patches. SovEmu applies them as the game
loads and never writes the library. Put a patch where the scanner looks for it:

```
roms/snes/Mother 2 (Japan).sfc
roms/snes/Mother 2 (Japan).bps                 # beside it, same name: applied by default
roms/snes/Mother 2 (Japan)/English v1.1.bps    # in a folder named as the game: to choose
roms/snes/Mother 2 (Japan)/Hard Mode.ips
```

The game panel shows a **Patch** choice when a game has any: the original, or one of its patches,
per player. A patched game keeps its own saves (`Mother 2 (Japan) [English v1.1].srm`), as it is in
effect another game. UPS and BPS patches carry checksums, so one made for another dump of the
game is refused with a message rather than applied wrongly; IPS has none. Disc images are not
patched.

## Game information

The **Library** page (admins) fetches titles, descriptions, release dates, developers,
publishers, genres, player counts, ratings and box art. The information is stored as each game
is looked up. Box art follows in the background, only for games with no thumb of their own,
`SOVEMU_ARTWORK_DOWNLOADS` at a time. Sources, in order:

1. **FinalBurn Neo's game lists**, for arcade and Neo Geo sets. No account needed:
   `mslug` becomes *Metal Slug - Super Vehicle-001* (1996, Nazca).
2. **The [LaunchBox Games Database](https://gamesdb.launchbox-app.com)**, by name and platform,
   with community ratings. No account needed, on by default. LaunchBox publishes the whole
   database as one file (`Metadata.zip`, about 110 MB). The server downloads it into
   `data/metadata/launchbox/` on the first lookup, and looks for a newer one when a lookup
   starts and the last check is a week old (`SOVEMU_LAUNCHBOX_REFRESH_HOURS`).
3. **[ScreenScraper](https://www.screenscraper.fr)**, the database Recalbox, Batocera, RetroBat and
   ES-DE scrape from. It matches by MD5, size and file name. It needs developer credentials
   (requested on its forum); a free user account raises the daily quota.
4. **[TheGamesDB](https://thegamesdb.net)**, by name and platform. It needs an API key (requested
   on its forum).

ScreenScraper and TheGamesDB are only asked about games the sources before them left without a
description, release date or box art, so their quotas go on what LaunchBox doesn't know (hacks,
translations, homebrew). Add credentials in `.env` (`SOVEMU_SCREENSCRAPER_*`,
`SOVEMU_THEGAMESDB_KEY`). A job stops
using a service that rejects credentials or runs out of quota, and retries those games next time.
Games no service found are asked again once a service they were not asked is configured, so
adding credentials later fills in the library on the next scan, without fetching everything again.

Supported systems: NES, SNES, Game Boy, Game Boy Color, GBA, Mega Drive, Master System, Game
Gear, PlayStation, and arcade / Neo Geo (FBNeo). N64, Dreamcast, GameCube and PS2 need GPU cores,
which SovEmu doesn't run yet, so they don't appear.

## Configuration

Every setting is an environment variable. There's nothing to set up by clicking through the app.
`compose.yaml` loads `.env` whole, so put settings there; `.env.example` lists them all. The same
names work as `--flags` on `sovemu-server`. An empty value means "unset". Each secret can instead
be read from a file named by `<VAR>_FILE`, for Docker or Kubernetes secrets. The server logs its
effective configuration at startup, with secrets redacted.

| Variable | Default | What |
|---|---|---|
| **Accounts** | | |
| `SOVEMU_ADMIN_USERNAME`, `SOVEMU_ADMIN_PASSWORD` | unset | The admin. Created at startup, and the password follows this value (changing it signs old sessions out). Without it, the first visit asks for an admin. |
| `SOVEMU_USERS__<n>__USERNAME`, `__PASSWORD` (or `__PASSWORD_FILE`), `__ADMIN` | none | More accounts, kept in step like the admin (see below) |
| `SOVEMU_USERS_PRUNE` | off | Delete accounts not declared above at startup |
| `SOVEMU_SESSION_DAYS` | `30` | How long a sign-in lasts |
| `SOVEMU_LOGIN_MAX_FAILURES` | `5` | Failed sign-ins before a username is locked out (0: never) |
| `SOVEMU_LOGIN_LOCKOUT_MINUTES` | `5` | How long the lockout lasts |
| **Library** | | |
| `SOVEMU_DATA_DIR` | `data` (`/data`) | Database, saves, cache, mirror |
| `SOVEMU_ROMS_DIR` | `<data>/roms` | The library, one folder per system |
| `SOVEMU_BIOS_DIR` | `<data>/bios` | BIOS files, one folder per system (see [BIOS files](#bios-files)) |
| `SOVEMU_LIBRARY_MODE` | `direct` | `direct`, `cache` or `mirror` (see above) |
| `SOVEMU_CACHE_GB` | `20` | `cache` mode: GiB kept locally |
| `SOVEMU_MIRROR_SYSTEMS` | all | `mirror` mode: comma-separated system ids |
| `SOVEMU_SCAN_INTERVAL_MINUTES` | `60` | Rescan period (0: startup only) |
| `SOVEMU_MIRROR_INTERVAL_MINUTES` | `360` | `mirror` mode: sync period (0: startup only) |
| `SOVEMU_CORES_DIR` | `cores` (`/opt/sovemu/cores`) | libretro cores |
| `SOVEMU_CACHE_LARGE_GAME_MB` | `64` | `cache` mode: larger games are cached after their session, not before |
| `SOVEMU_COPY_THREADS` | `4` | Files copied at once (mirror sync, artwork cache) |
| `SOVEMU_SYSTEMS__<id>__CORE`, `__ENABLED`, `__OPTIONS__<name>` | none | Per-system core, hiding a system, core options (see below) |
| `SOVEMU_CONTROLLERS__<n>__ID`, `__BINDINGS`, `__NAME` | none | Default button mapping for a controller model (`xbox`, `dualsense`, `pad:<vendor>:<product>`, `keyboard` …), as `south:B,east:A,…`: see [docs/CONTROLLERS.md](docs/CONTROLLERS.md). Wins over the admin page; a user's own mapping wins over both |
| **Game information** | | |
| `SOVEMU_METADATA_AUTO` | `true` | Look up new games after every scan |
| `SOVEMU_LAUNCHBOX` | `true` | Use the LaunchBox Games Database dump |
| `SOVEMU_LAUNCHBOX_REFRESH_HOURS` | `168` | Look for a newer dump when the last check is this old (`0`: never) |
| `SOVEMU_LAUNCHBOX_URL`, `SOVEMU_LAUNCHBOX_IMAGES_URL` | LaunchBox's | The dump and the image server for its box art |
| `SOVEMU_SCREENSCRAPER_DEVID`, `_DEVPASSWORD` | unset | ScreenScraper developer credentials |
| `SOVEMU_SCREENSCRAPER_USER`, `_PASSWORD` | unset | ScreenScraper account (raises the quota) |
| `SOVEMU_THEGAMESDB_KEY` | unset | TheGamesDB API key |
| `SOVEMU_METADATA_REGIONS` | `us,wor,eu,ss,uk,jp` | Region preference for titles, dates and box art |
| `SOVEMU_METADATA_LANGUAGES` | `en,wor` | Language preference for descriptions |
| `SOVEMU_SCREENSCRAPER_DELAY_MS` | `1200` | Pause between ScreenScraper requests |
| `SOVEMU_ARTWORK_DOWNLOADS` | `4` | Box art downloaded at once, in the background after a lookup |
| `SOVEMU_SCREENSCRAPER_URL`, `SOVEMU_THEGAMESDB_URL` | the services | API base URLs (mirrors, tests) |
| **Streaming** | | |
| `SOVEMU_HTTP` | `0.0.0.0:8080` | Web client and API address |
| `SOVEMU_UDP_PORTS` | `50000-50009` | One UDP port per concurrent session; publish the range |
| `SOVEMU_UDP_BIND` | `0.0.0.0` | Address the session ports bind to |
| `SOVEMU_ADVERTISE_IP` | detected (`127.0.0.1` in compose) | The address browsers send media to |
| `SOVEMU_ICE_SERVERS` | `[]` | STUN/TURN for browsers, as JSON `RTCIceServer`s |
| `SOVEMU_VIDEO_KBPS` | `2500` | Video bitrate cap (adaptation stays below it) |
| `SOVEMU_AUDIO_KBPS` | `128` | Opus bitrate |
| `SOVEMU_IDLE_TIMEOUT` | `300` | Seconds a session survives with no client |
| `SOVEMU_WORKER_START_TIMEOUT` | `120` | Seconds a game may take to load (disc images on a slow share) |
| `SOVEMU_WORKER_STOP_TIMEOUT` | `15` | Seconds a game may take to write its saves and exit before it is killed |
| `SOVEMU_REWIND_SECONDS` | `60` | Rewind history (0: rewind off) |
| `SOVEMU_REWIND_MAX_MB` | `64` | Memory cap for one session's rewind history |
| `SOVEMU_FAST_FORWARD` | `4` | Fast-forward speed |
| `SOVEMU_KEYFRAME_INTERVAL` | `600` | Frames between video keyframes |
| `SOVEMU_HOME_FEATURED` | `8` | Games in the home screen's hero (0 hides it) |
| `SOVEMU_HOME_ROW` | `20` | Games in each home screen row (recently played, recently added) |
| `SOVEMU_GPU` | `auto` | 3D rendering: `auto` (first working GPU, else software), `software`, `off`, or a render node (`/dev/dri/renderD128`) |
| **Operations** | | |
| `SOVEMU_SECURE_COOKIES` | off | Set when served over HTTPS |
| `SOVEMU_LOG_FORMAT` | `text` | Log lines on stderr: `text`, or `json` (one object per line, with the request's or session's fields) for a log collector; workers follow it |
| `SOVEMU_OTEL_ENDPOINT` | unset | An OpenTelemetry collector's OTLP/HTTP address (`http://otel-collector:4318`): traces and metrics are exported there. Off when unset. The standard `OTEL_*` variables apply on top (`OTEL_SERVICE_NAME`, `OTEL_RESOURCE_ATTRIBUTES`, `OTEL_EXPORTER_OTLP_HEADERS`, `OTEL_METRIC_EXPORT_INTERVAL`) |
| `SOVEMU_WEB_DIR`, `SOVEMU_WORKER` | set in the image | The built web client; the worker binary |
| `RUST_LOG` | `info` | Log filter |
| **compose.nas.yaml only** | | |
| `SOVEMU_NAS_HOST`, `SOVEMU_NAS_PATH`, `SOVEMU_NAS_USER`, `SOVEMU_NAS_PASSWORD` | | The SMB share to mount read-only |
| **compose.gpu.yaml only** | | |
| `SOVEMU_RENDER_GID` | | The host group that owns `/dev/dri/renderD*` |

Game workers start with an empty environment, apart from `PATH`, `HOME`, `TMPDIR`, `LANG` and
`RUST_LOG`. Cores are third-party code, so they never see credentials.

### Lists and maps

Accounts and per-system settings don't fit in one variable each. They use the `__` hierarchy
Sovereign uses for its own settings:

```bash
SOVEMU_USERS__0__USERNAME=kid
SOVEMU_USERS__0__PASSWORD_FILE=/run/secrets/kid     # or __PASSWORD
SOVEMU_USERS__1__USERNAME=dad
SOVEMU_USERS__1__PASSWORD=...
SOVEMU_USERS__1__ADMIN=true
SOVEMU_USERS_PRUNE=true                             # nobody else keeps an account

SOVEMU_SYSTEMS__psx__OPTIONS__pcsx_rearmed_cd_readahead=512   # libretro core option
SOVEMU_SYSTEMS__nes__CORE=nestopia                  # use this core (if installed)
SOVEMU_SYSTEMS__gamegear__ENABLED=false             # hide a system
```

Declared accounts are created at startup and kept in step: a changed password signs that user
out, and the admin flag follows the variable. An unknown system id, or a mistyped setting, stops
the server at startup with the variable's name.

### Kubernetes

[`charts/sovemu`](charts/sovemu) is a Helm chart that turns its values into these variables, so a
deployment is reproduced from one values file and its secrets. It covers:

- `settings` (any variable), `secrets` or `existingSecret`;
- `admin`, `users` (each with a password or a `passwordSecret`), `pruneUsers`, `systems`, and
  `iceServers`;
- the library as a read-only NFS volume, an existing claim or a host path;
- a persistent `/data`;
- an Ingress (TLS turns on secure cookies).

The media ports are a UDP `LoadBalancer` Service (MetalLB at home; `media.advertiseIP`, or the
pinned `loadBalancerIP`), or `media.hostNetwork`. The pod runs as uid 1000 with a read-only root
filesystem and no capabilities. On SIGTERM the server stops every session cleanly before
exiting, so saves are written.

```bash
helm install sovemu charts/sovemu -f my-values.yaml
```

`charts/sovemu/ci/full-values.yaml` uses every option.

### Health and sign-in

`GET /health` answers `ok` while the database is reachable. Sign-in locks a username out after
`SOVEMU_LOGIN_MAX_FAILURES` failed attempts, for `SOVEMU_LOGIN_LOCKOUT_MINUTES`.

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
