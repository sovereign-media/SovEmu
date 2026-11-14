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
- [ ] OIDC sign-in (deferred to Phase 6)
- [ ] Connect the NAS library (`nas.blh.casa/emulation`, SMB with credentials)

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

- [ ] `SovEmu.Android` (Kotlin) then `SovEmu.iOS` (Swift), as submodules (and move `SovEmu.Web`
      to its own repository as a submodule at the same time): touch overlay,
      physical controllers, app lifecycle

## Phase 5: features

- [ ] Per-game controller remapping, client-side shaders
- [ ] Rewind (ring buffer of compressed state deltas) and fast-forward
- [ ] Cheats UI, RetroAchievements (rcheevos; needs `SET_MEMORY_MAPS`)

## Phase 6: hardening

- [ ] TURN (coturn), H.264 fallback negotiation, metrics, worker sandboxing

## Later

- Multiplayer and spectators; optional pixel-perfect raw-frame DataChannel mode for LAN
