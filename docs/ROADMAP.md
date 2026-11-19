# SovEmu roadmap

Tracked in sovereign-media/SovEmu#1. The design is in [ARCHITECTURE.md](ARCHITECTURE.md).

## Phase 0: headless spike (done)

Prove the FFI and the run loop before any networking.

- [x] Cargo workspace under `SovEmu.Backend/`, crates in Sovereign's `SovEmu.<Name>/` layout
- [x] `SovEmu.LibretroFfi`: bindgen over the vendored `libretro.h`, plus a C shim for the
      variadic log callback
- [x] `SovEmu.RetroHost`: `Core::load` / `load_game` / `run_frame`, environment callback
      (pixel formats, directories, logging, core options, geometry/AV changes, input bitmasks),
      save states, reset, cheats, one-core-per-process guard
- [x] `SovEmu.TestCore`: a deterministic cdylib core so tests dlopen a real library with no
      downloads
- [x] `sovemu-worker headless`: PNG frames, `audio.wav`, `summary.json` (per-frame core time,
      audio sample accounting), scripted input (`--hold`), real-time pacing (`--realtime`)
- [x] `scripts/fetch-cores.sh` for local testing against real cores
- [x] Self-contained Alpine image: cores built from source at pinned commits
      (`scripts/cores.list`), musl dynamically linked worker, every core probed at build time
- [x] CI: fmt, clippy `-D warnings`, tests on Linux and macOS; tests on musl and the image build

Measured on an M-series Mac with fceumm (nightly) and the 240p Test Suite: 0.15 ms mean per
`retro_run` (p99 0.20 ms), audio within 0.004% of the expected sample count; real-time pacing
held 59.93 fps over 300 frames with no resyncs.

## Phase 1: first pixels in a browser

- [x] `SovEmu.Media`: RGBA→I420, VP8 (libvpx, real-time CBR, error resilient, keyframes on
      request), Opus (restricted low delay, 10 ms packets), resampling any core rate to 48 kHz
      with `rubato`
- [x] `SovEmu.Protocol`: binary input packet (sequence number, client time, full button state per
      port) and its ack; JSON session descriptions for non-trickle signalling
- [x] `sovemu-worker serve`: `str0m` peer on one UDP port, VP8 and Opus tracks, unordered
      no-retransmit input DataChannel, playout-delay hint, real-time loop on the `Pacer`;
      emulation pauses while no client is connected
- [x] `SovEmu.Web`, the first client: Astro + Tailwind, built to static files served by the same
      Rust process as `/api/offer`; keyboard and Gamepad API input, pixelated scaling, live
      latency stats (input round trip, network RTT, jitter buffer, decode)
- [x] End-to-end test: a str0m client receives and decodes VP8 and Opus from `serve` over UDP and
      gets acks for its input
- [x] Image: web client built in a Node stage; libvpx and opus in the runtime; ports 8080/tcp and
      50000/udp
- [ ] Measure button-to-photon latency in a real browser against a real core (needs a person and
      a camera, or a photodiode)

## Phase 2: server and library

- [x] `SovEmu.Server`: axum serving `SovEmu.Web` and `/api/*`; SQLite (sqlx, WAL, embedded
      migrations)
- [x] Auth: local accounts (argon2, HttpOnly session cookie), first-run admin setup, admin adds
      users
- [x] ROM library: systems table (Recalbox folder names and aliases, extension fallback, preferred
      cores), scanner that keeps game ids stable across rescans, No-Intro title and tag parsing
- [x] Sessions: one `sovemu-worker session` per game, JSON-line IPC with request ids, a UDP port per
      session from a pool, one session per user, idle timeout, clean shutdown, play history
- [x] Web: sign-in and first-run setup, a system carousel and game list in the style of console
      frontends (gamepad and keyboard navigation, hint bar), the player with a splash and an in-game
      menu (resume, reset, stats, quit)
- [x] `scripts/fetch-free-roms.sh`: pinned, freely licensed homebrew for NES, SNES, GB, GBC and GBA
- [x] Verified in Docker: Snes9x streaming *Castle Platformer* through the server to a WebRTC client
- [x] Configuration entirely through environment variables: the admin account
      (`SOVEMU_ADMIN_*`, kept in step on every start), `*_FILE` secrets, periodic rescans and
      mirror syncs, metadata after every scan, stream bitrates; empty values mean unset; the
      effective configuration logged (redacted); compose loads `.env` whole; workers start with a
      cleared environment
- [ ] OIDC sign-in (deferred to Phase 6)
- [x] NAS library: `compose.nas.yaml` mounts the SMB share read-only (credentials in a gitignored
      `.env`); Batocera artwork (box art, image, marquee, video snaps) is scanned and served with
      range support; the game list shows it; scans run in the background, in parallel
      (3,497 games over SMB in ~20 s)
- [x] PlayStation (PCSX-ReARMed, CHD) and arcade / Neo Geo (FBNeo) cores; `SOVEMU_SYSTEM_DIR` for BIOS
- [x] Remote library (Cloudflare Zero Trust, ~100 ms per request, a few MB/s): a local LRU cache
      of played games and artwork (`SOVEMU_CACHE_GB`), disc images cached only after their first
      session, system artwork prefetched, PlayStation CD read-ahead raised to 256 sectors, worker
      load deadline raised to 2 minutes
- [x] Library modes: `direct` (NAS on the LAN, the default), `cache` (remote NAS), `mirror`
      (a full local copy synced in the background, optionally per system), with a Library page
- [x] Game information: FBNeo DATs (arcade and Neo Geo, no account; all 502 sets named on the
      NAS), ScreenScraper (MD5 + size + name) and TheGamesDB (name + platform) clients, merged into
      `game_metadata`; missing box art downloaded; descriptions, dates, developers, genres and
      players in the game list
- [x] LaunchBox Games Database as the default source: its dump downloaded and refreshed by the
      server, matched by name and alternate names per platform, with community ratings
- [ ] Live-test the ScreenScraper and TheGamesDB clients once credentials exist (tested against
      mock services built from ES-DE's parsing code)
- [x] Group multi-disc games (`(Disc N)`) into one entry: the scanner folds them (`game_discs`),
      the worker plays an `.m3u` of the discs and changes disc through libretro's disk control
      (`setDisc`), and the in-game menus offer Change disc
- [x] Favourites and per-user collections: a heart and a collections menu on the game panel, a
      Favourites filter, and Home rows for favourites and each collection
- [x] Search across every system (`Query.search`: every word, case and accents folded, titles
      starting with the query first), from a Search entry in the navigation and X on Home

## Phase 3: persistence

- [x] Battery saves: `RETRO_MEMORY_SAVE_RAM` restored from `<game>.srm` at load, written when changed
      (checked every 10 s) and on exit, atomically; RetroArch-compatible names and format
- [x] Save states in slots with PNG screenshots; slot 0 is the auto state, written whenever a
      session ends (quit, closing the tab, idle timeout)
- [x] API: list, save, load and delete states; screenshots
- [x] Web: the splash offers Continue (from the auto save) or Start over; the in-game menu has Save
      state and Load state pickers with thumbnails and times
- [x] Verified in Docker: *Nova the Squirrel* wrote its `.srm`, a slot and the auto state with screenshots

## Phase 4: mobile clients

- [x] `SovEmu.Android` (sovereign-media/SovEmu.Android, submodule): Kotlin + Compose; `core` module
      with the protocol and API models (shared byte vectors); sign-in, system carousel, game list,
      player (Stream libwebrtc), gamepad by position and a touch controller, hotkeys, in-game menu
      (save/load slot 1, continue, quit); CI builds a debug APK
- [x] `SovEmu.iOS` (sovereign-media/SovEmu.iOS, submodule): SwiftUI + XcodeGen; `SovEmuKit` package
      (Swift 6) with the protocol and API (shared byte vectors); the same screens and player
      (stasel/WebRTC, GameController); CI runs the package tests and a simulator build
- [ ] Run both on devices against a real server (not yet: no device or emulator on the build machine)
- [ ] Parity with the web player: save-slot pickers with thumbnails, cheats, remapping, filters
- [ ] Move `SovEmu.Web` to its own repository once the image takes a released web build

## Phase 5: features

- [x] Fast-forward (4 frames per tick) and rewind (ring of states every 2 frames, capped at 64 MiB
      / 60 s) while a hotkey is held: Tab / Backspace, or Select+R / Select+L on a gamepad (those
      buttons are kept from the game meanwhile). A `0x02` hotkeys message on the input channel,
      refreshed while held and dropped by the worker after 500 ms without a refresh. RTP timestamps
      follow wall-clock ticks, not emulated frames
- [x] The in-game menu pauses the game: a pause bit in the `0x02` hotkeys message, held while the
      menu is open and refreshed like the others, stops the worker running frames (web and Android)
- [x] Cheats per user and game (stored in SQLite, applied by the worker at start and on every change)
- [x] Controller remapping, keyboard and gamepad, per browser
- [x] Display filters on a WebGL canvas: sharp pixels, smooth, CRT (scanlines + aperture mask)
- [x] A home screen in Sovereign's style: a hero carousel of featured games (a new pick each day,
      games with artwork and descriptions first) over rows of continue playing (the auto save's
      screenshot), systems and recently added, from one `GET /api/home`
- [x] The web client on React islands with Radix UI primitives and Lucide icons
- [x] Verified: fast-forward over WebRTC ran 216 emulated frames/s against 61 normally
- [ ] RetroAchievements (rcheevos in the worker, an RA account per user). Memory-region access
      (`Core::system_ram`) is in place for it

## Phase 6: hardening

- [x] Adaptive bitrate: str0m's send-bandwidth estimate (TWCC/REMB) drives the VP8 target (85% of
      the estimate minus audio, between 300 kbit/s and the configured rate, changes over 10% only)
- [x] STUN/TURN: `SOVEMU_ICE_SERVERS` is handed to browsers through `/api/config`
- [x] Sign-in throttling: five failures lock a username out for five minutes
- [x] `GET /health`; Prometheus `GET /metrics` behind a bearer token (off by default)
- [x] Security headers (nosniff, frame denial, referrer and permissions policies); compose healthcheck
- [x] Configuration reference in the README
- [x] Configuration entirely through environment variables: the admin account
      (`SOVEMU_ADMIN_*`, kept in step on every start), `*_FILE` secrets, periodic rescans and
      mirror syncs, metadata after every scan, stream bitrates; empty values mean unset; the
      effective configuration logged (redacted); compose loads `.env` whole; workers start with a
      cleared environment
- [x] The whole deployment from configuration, as in Sovereign: declared accounts
      (`SOVEMU_USERS__<n>__*`, optional pruning), per-system cores, core options and hiding
      (`SOVEMU_SYSTEMS__<id>__*`), and every tunable that was a constant (session length, sign-in
      lockout, worker start timeout, rewind, fast-forward, keyframes, cache threshold, copy
      threads, metadata regions and languages, ScreenScraper pacing)
- [x] Helm chart (`charts/sovemu`): values become the environment; secrets or existing Secrets;
      NFS, claim or host-path library; UDP LoadBalancer or host networking for media; Ingress;
      read-only root, uid 1000, no capabilities. Linted and schema-checked in CI
- [x] Graceful shutdown on SIGTERM (Kubernetes, `docker stop`): every session saves first
- [ ] OIDC sign-in
- [ ] H.264 fallback for clients without VP8 decode
- [ ] Worker resource limits (cgroups/rlimits per session)

## Phase 7: 3D systems

- [x] Headless OpenGL in the frontend: `SET_HW_RENDER` (OpenGL, core profile, GLES), an EGL
      context on a framebuffer object, frames read back after `retro_run`; libEGL opened at run
      time. The test core has a GL mode; the image's test stage renders it with llvmpipe
- [x] GPU detection as Sovereign does it: `sovemu-worker gpu` draws on every EGL device and the
      server keeps what works (`SOVEMU_GPU`: auto, software, off, a render node), logging a
      summary; `GET /api/gpu`; systems report `rendering`
- [x] N64 (Mupen64Plus-Next), Dreamcast (Flycast), GameCube (Dolphin, with its `Sys` overlaid
      on the BIOS folder) and PS2 (PCSX2), built from source on Alpine
- [x] Analog sticks and triggers: input message `0x03` in Rust, TypeScript, Kotlin and Swift;
      the left stick stops doubling as the D-pad for 3D systems
- [x] `compose.gpu.yaml`; Helm `gpu.resources` / `gpu.hostDevice`
- [x] Loading progress: the worker reports its phases and the bytes it has read (`progress`
      before `ready`); clients follow a start with `start_id` and
      `GET /api/sessions/starting/{id}`; the web player shows a loading bar
- [x] Emulator settings in the web app: each core's options (probed by a worker), changed per
      system by admins (`system_settings`, migration 0005); the environment wins and is shown
      locked; changes are offered as `SOVEMU_SYSTEMS__…` lines
- [x] Core options v1/v2 in RetroHost (`GET_CORE_OPTIONS_VERSION` = 2): the settings page groups
      each system's settings by the core's categories (else by keyword) under a system dropdown,
      and shows the core's explanation of each
- [x] The image moves from Alpine to Debian (trixie), so NVIDIA's own driver works through the
      NVIDIA container toolkit (`compose.nvidia.yaml`, the chart's `gpu.runtimeClassName`); one
      image for every host. GPUs without a DRM node are used too
- [x] Cores prebuilt from the libretro stable release (1.22.2, pinned by SHA-256) instead of
      compiled from source; the image is x86_64 only
- [x] The API is GraphQL only (async-graphql; `schema.graphql` checked in; the web client typed
      by codegen; Android and iOS moved to it); files stay plain HTTP under `/media/`
- [x] Games as a grid of covers in Sovereign's style: Sort, Filters (a drawer) and Jump (a letter
      carousel) in a chip strip, and a side panel per game with its own URL (`?game=`)
- [ ] Vulkan cores (paraLLEl-RDP, Dolphin's Vulkan backend)
- [ ] Hardware video encoding (VA-API) when a GPU is present

## Later

- Multiplayer and spectators; optional pixel-perfect raw-frame DataChannel mode for LAN
