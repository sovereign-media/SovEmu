# SovEmu roadmap

Planned in sovereign-media/SovEmu#1 (closed 2026-11-27); each open item below links its own issue. The
design is in [ARCHITECTURE.md](ARCHITECTURE.md).

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
- [x] `SovEmu.Web`, the first client: Astro + Tailwind (now Vite + TanStack Router), built to static files served by the same
      Rust process as `/api/offer`; keyboard and Gamepad API input, pixelated scaling, live
      latency stats (input round trip, network RTT, jitter buffer, decode)
- [x] End-to-end test: a str0m client receives and decodes VP8 and Opus from `serve` over UDP and
      gets acks for its input
- [x] Image: web client built in a Node stage; libvpx and opus in the runtime; ports 8080/tcp and
      50000/udp
- [ ] Measure button-to-photon latency in a real browser against a real core (needs a person and
      a camera, or a photodiode): #278

## Phase 2: server and library

- [x] `SovEmu.Server`: axum serving `SovEmu.Web` and `/api/*`; SQLite (sqlx, WAL, embedded
      migrations)
- [x] Auth: local accounts (argon2, HttpOnly session cookie), first-run admin setup, admin adds
      users, changes their roles and passwords, signs them out everywhere and removes them (never
      the last admin, nor an account the environment declares, which shows locked)
- [x] The admin Activity page: who did what, and when (sign-ins and refusals through
      `sovemu_identity`'s observer, games started and ended through `sovemu_sessions`', admin
      changes from their resolvers), kept in `activity` (pruned after
      `SovEmu__ActivityLog__RetentionDays`), filtered by account and kind, paged, and exported as CSV
- [x] The admin Statistics page: every game played is kept as it ends (`play_sessions`),
      and a week, month or year shows the totals, time played per day (per
      week over a year), the most played games, each account's and system's time, and the most
      favourited games, each figure one query over the window by start time
- [x] The admin Logs page: the server's recent log kept in memory (`SovEmu__Logging__BufferLines`, 0
      off) by a `tracing` layer behind the same filter as stderr, with its game sessions' worker
      lines (through `sovemu_sessions`' observer) tagged by session; filtered by level, session
      and text, followed live and paused
- [x] The admin Overview, the menu's first page: who is playing and the game ports in use,
      the library and its last scan, game information, BIOS gaps, rendering, the disk, failed
      games and today's client errors, the version and uptime (`serverResources.startedAt`),
      each card linking to its page
- [x] Games needing attention (`/admin/attention/`): games without information or box art,
      duplicates (the same title and tags twice in a system), games missing a disc (the scan
      keeps `games.missing_parts`) and the files no system plays (`scan_skipped`, migration
      0025), each counted per system, and a game looked up again on its own (`lookUpGame`)
- [x] Shared collections (`/admin/collections/`): an admin's collections for everyone, on every
      Home beside each user's own (`sharedCollections`; one table, `collections.user_id` null),
      filled from a game's panel; declared as `SovEmu__Collections__<n>__Name`
      and `__GAMES__<m>` (`<system>/<file>`), kept in step after each scan and locked, and
      offered as environment lines
- [x] Backups (`/admin/backups/`): the database (`VACUUM INTO`), the signing keys and every save
      in one zip, made by the `Backup` job (`SovEmu__Backups__IntervalHours`, newest
      `SovEmu__Backups__Keep` kept, in `SovEmu__Backups__Directory`) or the page's button, downloaded through
      a signed URL for admins only; restored at startup from `SovEmu__Backups__RestoreFrom`, what it
      replaces moved into `before-restore-<time>/`, each backup restored once
- [x] The About page lists every open-source package shipped, by licence, with its text: the
      server's crates (cargo-about, `about.toml`'s accepted licences, listed again by the image's
      `notices` stage and read beside the server; `third-party-notices.json` committed for a
      local build) and the web client's
      npm packages (a Vite plugin with its own allow-list), through `thirdPartyNotices`
- [x] Admins see every browser and app signed in (a live refresh family: its client, when it
      signed in and was last used, when it lapses) on the Devices page and each account's own
      page, with what that account has played and its saves, and sign one out through oauth-as's
      family revocation (`signIns`, `user`, `endSignIn`)
- [x] The apps name their device once signed in (`nameDevice`: app, version, device), kept by
      refresh family in SovEmu's own `sign_in_devices` beside oauth-as's
      records, so a sign-in on the Devices page says which device it is
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
      (`SOVEMU_ADMIN_*`, kept in step on every start), secrets from the environment, periodic rescans and
      mirror syncs, metadata after every scan, stream bitrates; empty values mean unset; the
      effective configuration logged (redacted); compose loads `.env` whole; workers start with a
      cleared environment
- [x] OIDC sign-in (Phase 6)
- [x] NAS library: `compose.nas.yaml` mounts the SMB share read-only (credentials in a gitignored
      `.env`); scans run in the background, in parallel
      (3,497 games over SMB in ~20 s)
- [x] PlayStation (PCSX-ReARMed, CHD) and arcade / Neo Geo (FBNeo) cores; `SOVEMU_SYSTEM_DIR` for BIOS
- [x] Remote library (Cloudflare Zero Trust, ~100 ms per request, a few MB/s): a local LRU cache
      of played games (`SovEmu__Library__Cache__MaxGigabytes`), disc images cached only after their first
      session, PlayStation CD read-ahead raised to 256 sectors, worker
      load deadline raised to 2 minutes
- [x] Library modes: `direct` (NAS on the LAN, the default), `cache` (remote NAS), `mirror`
      (a full local copy synced in the background, optionally per system), with a Library page
- [x] Game information: FBNeo DATs (arcade and Neo Geo, no account; all 502 sets named on the
      NAS) and ScreenScraper (MD5 + size + name), merged into `game_metadata`; missing box art
      downloaded; descriptions, dates, developers, genres and players in the game list
- [x] ScreenScraper as the only online source, behind a provider interface (#207): LaunchBox and
      TheGamesDB removed, detailed lookup progress, partial refreshes by field, source and age
- [x] Artwork from ScreenScraper only: box art, screenshots (backdrops), logos (marquees) and
      videos downloaded as WebP (videos as MP4); the scan and the mirror read ROM folders only,
      never the artwork beside them
- [x] ESRB age ratings (badges of SovEmu's own), play modes, series and themes from ScreenScraper,
      and a Top rated row on Home
- [ ] Live-test the ScreenScraper client once credentials exist (tested against a mock service
      built from ES-DE's parsing code): status codes, quota fields, the GameCube and PS2 system
      ids, whether stored box art URLs expire: #279
- [x] Group multi-disc games (`(Disc N)`) into one entry: the scanner folds them (`game_discs`),
      the worker plays an `.m3u` of the discs and changes disc through libretro's disk control
      (`setDisc`), and the in-game menus offer Change disc
- [x] Multi-part media named every common way: Redump/No-Intro `(Disc N)`, `(Disk N)`, `(Disk A)`;
      TOSEC `(Disk 1 of 2)`, `(Tape 2 of 2 Side B)`, `(Side A)`, `(Part 1 of 3)`; `[Disc 1]`,
      `(CD1)` and a title's `- Disc 1`. The kind (`Game.partKind`) picks the grid badge's icon
- [x] Favourites and per-user collections: a heart and a collections menu on the game panel, a
      Favourites filter, and Home rows for favourites and each collection
- [x] Search across every system (`Query.search`: every word, case and accents folded, titles
      starting with the query first), from a Search entry in the navigation and X on Home
- [x] Release labels (`Game.release`): games showing the same title on one system are told apart
      by the tags they do not share ("Translation", "Rev 1"), else their file's title (a
      compilation's games), else their format (`.z64` beside `.v64`)
- [x] Soft patches (IPS, UPS, BPS): beside a game with its name (applied by default) or in a
      folder named as it; chosen per player on the game panel, applied to a copy as the game
      loads, with the patched game's own saves
- [x] Decorations (bezels), server side: Batocera-layout sets from `SovEmu__Decorations__Directories`,
      `data/decorations` and the built-in `sovemu` set (drawn by the server's code), merged;
      chosen with `SovEmu__Decorations__Set` / `SovEmu__Systems__<id>__Decoration`; `Game.decoration`
- [x] Decorations drawn by the web player (framed only when the screen is wider than the picture)
- [x] Decorations drawn by the Android and iOS players

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
- [ ] Run both on devices against a real server: Android is played on Android TV; iOS is #286
- [x] Parity with the web player: save-slot pickers with thumbnails, cheats, remapping, filters
- [ ] Move `SovEmu.Web` to its own repository once the image takes a released web build: #285

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
      (`Core::system_ram`) is in place for it: #280

## Phase 6: hardening

- [x] Adaptive bitrate: a stream starts at `SovEmu__Streaming__Video__Kbps` and moves between
      `SovEmu__Streaming__Video__MinKbps` and `SovEmu__Streaming__Video__MaxKbps`. str0m's send-bandwidth estimate
      (TWCC/REMB) drives the target (85% of the estimate minus audio, changes over 10% only), and
      the stats every client sends once a second lower it while the client's decoder falls
      behind. Encoders that reopen to change rate (VA-API, NVENC) are raised less often
- [x] STUN (Cloudflare's and Google's free servers): each session
      offers its public address and runs full ICE, so players away from home connect with no
      port forwarded; and a built-in TURN relay (always on, port 3478, `SovEmu.Relay`, #50),
      the only TURN server, with per-player credentials, relaying only to the server's sessions
- [x] Sign-in throttling: five failures lock a username out for five minutes
- [x] `GET /health` for probes
- [x] Structured logs (`SovEmu__Logging__Format=json`): every API request in a `request` span (route,
      status, GraphQL operation) and every game session in a `session` span, whose id the
      worker also logs with
- [x] OpenTelemetry traces and metrics over OTLP/HTTP (`SovEmu__Telemetry__Otlp__Endpoint`, off by default):
      request spans, session spans, request durations, sessions started and their length,
      sessions running, capacity, accounts and games per system. This replaced the Prometheus
      `/metrics` route
- [x] Signed file URLs, as Sovereign's: every `/media/…` URL the API hands out carries an
      HMAC-SHA256 signature over the user, path and an hour-rounded expiry (`SovEmu__Assets__SecretKey`,
      else generated into `data/server.json`; `SovEmu__Assets__UrlHours`); `/media/` serves only
      signed URLs, as the signed user, with no cookie
- [x] Security headers (nosniff, frame denial, referrer and permissions policies); compose healthcheck
- [x] Configuration reference in the README
- [x] Configuration entirely through environment variables: the admin account
      (`SOVEMU_ADMIN_*`, kept in step on every start), secrets from the environment, periodic rescans and
      mirror syncs, metadata after every scan, stream bitrates; empty values mean unset; the
      effective configuration logged (redacted); compose loads `.env` whole; workers start with a
      cleared environment
- [x] The whole deployment from configuration, as in Sovereign: declared accounts
      (`SovEmu__Users__<n>__*`, optional pruning), per-system cores, core options and hiding
      (`SovEmu__Systems__<id>__*`), and every tunable that was a constant (session length, sign-in
      lockout, worker start timeout, rewind, fast-forward, keyframes, cache threshold, copy
      threads, metadata regions and languages, ScreenScraper pacing)
- [x] Helm chart (`charts/sovemu`): values become the environment; secrets or existing Secrets;
      NFS, claim or host-path library; UDP LoadBalancer or host networking for media; Ingress;
      read-only root, uid 1000, no capabilities. Since moved out of this repository, to the
      cluster's charts
- [x] Graceful shutdown on SIGTERM (Kubernetes, `docker stop`): every session saves first
- [x] Sign-in: `SovEmu.Identity`, the `oauth-as` OAuth 2.1 server stored in SQLite (conformance
      harness passed) with OpenID Connect over it (discovery, JWKS, ES256 ID tokens, userinfo,
      end session). Three clients with per-install ids, as Sovereign's: web (code flow with
      PKCE, no password grant), device (the device grant, or the password fallback,
      `SovEmu__Authentication__PasswordSignIn`) and mobile (the system browser's code flow). Bearer tokens for the
      API; roles `viewer` and `admin`
- [x] The apps show a sign-in code (the device grant)
- [ ] Signing in with another provider (Google, Authentik, …) as well as local accounts: #281,
      and #238 for what Sovereign's apps expect of sign-in
- [x] H.264, preferred when the client offers it (VP8 otherwise; `SovEmu__Streaming__Video__Codec`): libavcodec
      over VA-API, NVENC or x264 (`SovEmu__Streaming__Video__Encoder`), falling back to x264 when a GPU
      encoder does not open, fails, or holds frames back. VP8 stayed the default
      (`SovEmu__Streaming__Video__Codec=vp8`) after a TV's hardware H.264 decoder measured six times slower
      than its VP8 one; a client stating which codec it decodes fastest is the follow-up
- [x] How each stream is made, shown to players and admins: the worker's pipeline (renderer,
      codec, encoder and its fallbacks with why, audio, run-ahead) goes to the client on the input
      channel (`0x82`) for its stats panel, and to the server, whose admin Sessions page lists
      every game being played with the worker's live numbers (`activeSessions`)
- [x] Admins stop any session (`stopSession`, saves written first) and see the last sessions
      that failed (`sessionFailures`, `SovEmu__Sessions__FailuresKept` kept in memory): starts the
      server refused, loads that failed, crashes and killed workers, with the worker's last lines
- [x] The admin Streaming page: a connection test from the admin's browser to a game port
      (`testConnection`: a `sovemu-worker connection-test` peer with no media on a pooled port,
      echoing a data channel), showing whether the ports are reachable, the round trip, and
      whether it went direct, through STUN or through a TURN relay; then the stream settings
- [x] The admin Client errors page: what the apps report (`reportClientEvents`: failed or lost
      connections, stalled pictures, screen errors, unknown controllers) is kept in
      `client_events` (pruned after `SovEmu__ClientErrors__RetentionDays`) and grouped by
      kind and by app and device, each group opening onto its events with the game they
      happened in
- [x] A 3D core's frames converted to I420 on the GPU before they are read back (1.5 bytes a
      pixel, no CPU conversion or flip), and the RGBA copy vectorised: 25.5 ms to 6.2 ms a frame
      at GameCube 4× on llvmpipe ([research](research/gpu-frames-2026-11.md)), #283. Zero-copy
      to VA-API was left: SovEmu streams VP8, which encodes on the CPU
- [x] Android: ask the H.264 decoder for low-latency mode (Android 11+)
- [ ] Worker resource limits (cgroups/rlimits per session): #282

## Phase 7: 3D systems

- [x] Headless OpenGL in the frontend: `SET_HW_RENDER` (OpenGL, core profile, GLES), an EGL
      context on a framebuffer object, frames read back after `retro_run`; libEGL opened at run
      time. The test core has a GL mode; the image's test stage renders it with llvmpipe
- [x] GPU detection as Sovereign does it: `sovemu-worker gpu` draws on every EGL device and the
      server keeps what works (`SovEmu__Rendering__Gpu`: auto, software, off, a render node), logging a
      summary; `GET /api/gpu`; systems report `rendering`
- [x] N64 (Mupen64Plus-Next), Dreamcast (Flycast), GameCube (Dolphin, with its `Sys` overlaid
      on the BIOS folder) and PS2 (PCSX2), built from source on Alpine
- [x] Analog sticks and triggers: input message `0x03` in Rust, TypeScript, Kotlin and Swift;
      the left stick stops doubling as the D-pad for 3D systems
- [x] `compose.gpu.yaml`; the Helm chart's GPU resources or host device
- [x] Loading progress: the worker reports its phases and the bytes it has read (`progress`
      before `ready`); clients follow a start with `start_id` and
      `GET /api/sessions/starting/{id}`; the web player shows a loading bar
- [x] Emulator settings in the web app: each core's options (probed by a worker), changed per
      system by admins (`system_settings`); the environment wins and is shown
      locked; changes are offered as `SovEmu__Systems__…` lines
- [x] Run-ahead (#85): per system, the picture runs 0-4 frames ahead of the game, hiding its
      own input lag (single-instance: save, run ahead, show, restore; sound from the real
      frame). `SovEmu__Systems__<id>__RunAhead` or the admin (`system_run_ahead`, migration
      0017); 2D systems only, on by default (1 frame), and a worker that cannot keep up runs
      fewer frames ahead
- [x] Core options v1/v2 in RetroHost (`GET_CORE_OPTIONS_VERSION` = 2): the settings page groups
      each system's settings by the core's categories (else by keyword) under a system dropdown,
      and shows the core's explanation of each
- [x] The image moves from Alpine to Debian (trixie), so NVIDIA's own driver works through the
      NVIDIA container toolkit (`compose.nvidia.yaml`, the chart's `gpu.runtimeClassName`); one
      image for every host. GPUs without a DRM node are used too
- [x] Cores prebuilt from the libretro stable release (1.22.2, pinned by SHA-256) instead of
      compiled from source; the image is x86_64 only
- [x] The API is GraphQL only (async-graphql; `schema.graphql` checked in; the web client typed
      by codegen, now gql.tada; Android and iOS moved to it); files stay plain HTTP under `/media/`
- [x] Games as a grid of covers in Sovereign's style: Sort, Filters (a drawer) and Jump (a letter
      carousel) in a chip strip, and a side panel per game with its own URL (`?game=`)
- [ ] Vulkan cores (paraLLEl-RDP, Dolphin's Vulkan backend): #284
- [x] Hardware video encoding (VA-API) when a GPU is present: H.264 through libavcodec (Phase 6);
      without reading frames back is #283

## Later

- Multiplayer and spectators; optional pixel-perfect raw-frame DataChannel mode for LAN: #287
