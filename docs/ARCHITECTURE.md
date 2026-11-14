# SovEmu architecture

The v1 design and the decisions behind it, agreed in sovereign-media/SovEmu#1 (2026-11-13). The phases are in [ROADMAP.md](ROADMAP.md).

## Decisions

| Area | Decision |
|---|---|
| Systems (v1) | 2D/software-rendered cores only: NES, SNES, GB/GBC/GBA, Genesis, possibly PS1 (Beetle PSX, software renderer). No GPU cores (N64/PSP/Dreamcast). |
| Server | Linux, CPU only (no hardware encoder) |
| Scale | Personal/family, single node |
| Database | SQLite (WAL mode) via `sqlx` with migrations; large files stay on disk |
| Licensing | Non-commercial, so non-commercial cores (snes9x, genesis_plus_gx, etc.) are allowed |
| ROMs / BIOS | Shared library managed by the admin: the server scans a `roms/` folder and BIOS files live in `system/`. No user uploads. |
| Multiplayer | Single-player first. The data model allows multiple players and spectators later. |
| Mobile clients | Native Kotlin (Android) and Swift (iOS), both using libwebrtc |
| Auth | Local accounts (argon2) with admin invites, plus optional OIDC |
| Video | VP8 by default (libvpx realtime, encoded at the console's native resolution); H.264 (x264 `ultrafast` + `zerolatency`) as a negotiated fallback |
| Audio | Opus at 48 kHz, resampled from the core's own rate with `rubato` |
| Input | Unreliable, unordered WebRTC DataChannel; each packet carries a sequence number and the full button state |
| Designed for, not in v1 | Rewind/fast-forward, cheats, RetroAchievements (rcheevos), per-game controller remapping, client-side shaders |

## Architecture

**Constraint:** libretro cores keep their state in global variables, and their callbacks are bare C function pointers with no user-data pointer. Two instances of the same core can't share a process, and a crashing core takes its process down with it. So SovEmu runs **one worker process per game session**, managed by a supervisor server.

```
 Web / Android / iOS client
        │  HTTPS + WebSocket (auth, library, signaling)
        ▼
 ┌──────────────────────── sovemu-server (axum/tokio) ────────────────────────┐
 │ REST API · auth · ROM library · SQLite · session orchestrator · signaling │
 └──────────────┬─────────────────────────────────────────────────────────────┘
                │ spawn + IPC (Unix socket, framed messages)
                ▼
 ┌──────── sovemu-worker (1 per session) ────────┐
 │ libretro core (dlopen via libloading)         │
 │ frame loop → pixel convert → VP8/H.264 ───────┼──► WebRTC video track
 │            → resample → Opus ─────────────────┼──► WebRTC audio track
 │ input_state ◄── DataChannel (unreliable) ◄────┼─── gamepad / touch input
 │ SRAM / savestates ──► disk                    │
 └───────────────────────────────────────────────┘
```

- The worker owns the WebRTC peer ([`str0m`](https://github.com/algesten/str0m), sans-IO). The server only relays SDP offers/answers and ICE candidates over WebSocket, so it never handles media.
- Frames are encoded at native resolution (e.g. 256×224). Clients scale up with nearest-neighbor or a shader.
- The run loop must not assume one emulated frame per tick, so that fast-forward (run N frames, encode the last) and rewind (ring buffer of compressed state deltas) can be added later.
- NAT traversal: STUN, plus coturn for TURN when mobile networks need it.

### Repository layout

The repository mirrors `sovereign-media/sovereign`. The backend is in-tree under
`SovEmu.Backend/`, one directory per crate. Each client is a separate project at the root. The
native clients will be separate repositories checked out as unpinned submodules, so the
server/client boundary is also a git boundary. `SovEmu.Web`, the first client, lives in-tree
for now. It is self-contained (its own package.json, tests and build), so it can move to its own
repository later.

```
SovEmu/
  SovEmu.Backend/            Cargo workspace (built and tested from here)
    SovEmu.LibretroFfi/      bindgen over include/libretro.h + variadic log shim     [Phase 0]
    SovEmu.RetroHost/        safe frontend: load core, environment, run loop         [Phase 0]
    SovEmu.TestCore/         deterministic cdylib core for tests                     [Phase 0]
    SovEmu.Worker/           per-session binary (headless now, WebRTC later)         [Phase 0]
    SovEmu.Media/            RGBA→I420, VP8 (libvpx), Opus, resampling               [Phase 1]
    SovEmu.Protocol/         wire formats: input packets, signalling                 [Phase 1]
    SovEmu.Server/           axum: web client + API, auth, SQLite, orchestration     [Phase 2]
  SovEmu.Web/                Astro + Tailwind, static; served by the Rust server     [Phase 1]
  SovEmu.Android/            submodule: Kotlin + Compose, Stream libwebrtc, gamepad + touch
  SovEmu.iOS/                submodule: SwiftUI + XcodeGen, stasel/WebRTC, GameController + touch
  docs/  scripts/  .claude/  .github/
  cores/  roms/  system/     gitignored: fetched cores, admin ROM library, BIOS
```

Dependencies point one way. `Worker` uses `RetroHost` (which uses `LibretroFfi`), `Media` and
`Protocol`. `Media` knows nothing about libretro: it takes RGBA frames and interleaved samples.
`Protocol` is shared by `Worker` and `Server`. `Server` never links a core: it starts
`sovemu-worker` processes and talks to them over IPC.

### Server, sessions and the library

`sovemu-server` is the container's main process. It serves the web client and `/api/*`, owns
the SQLite database, scans the library, and runs game sessions. Each session is a
`sovemu-worker session` child process. The server sends it JSON-line requests on stdin
(`offer`, `reset`, `save_state`, `load_state`, `shutdown`, each with an id) and reads replies and
events from its stdout (`ready`, `answer`, `done`, `error`, `client_connected`,
`client_disconnected`). The worker exits when stdin closes, so a crashed server never leaves
orphans. It also exits on `shutdown` or after the idle timeout. A user has at most one session.
Each session gets one UDP port from a fixed range, so the container publishes that range.

Library folders follow the Recalbox/Batocera names (`nes/`, `snes/`, `megadrive/` …), plus
aliases (`genesis/`, No-Intro style names). A file outside a known folder is placed by its
extension, but only when exactly one system claims that extension. A rescan updates rows in place
by path, so game ids (and with them play history and saves) survive a rescan.

### A remote library

The library can sit on a share reached over a tunnel. That means about 100 ms per request and a
few MB/s. Three things keep the slowness away from playing:
- **Cartridge games:** a game is read into memory when it loads, so play itself never touches the
  network.
- **Disc games:** a disc is read during play on the core's own CD thread (PCSX-ReARMed, read-ahead
  256 sectors).
- **The cache:** `data/cache` holds what has been played and seen:
  - a game is served from its copy once it has been played;
  - small files are copied straight away; disc images are copied after their first session;
  - least recently used files go first;
  - files that refer to other files (`.cue`, `.m3u`) stay on the share.

A copy counts as current when its size matches the original's and its modification time is within
2 s. Filesystems store these times at different precisions.

### Saves

Saves belong to a user and live under `/data/saves/<user id>/<system>/`.
- **Battery saves** are `<game>.srm`. The worker restores one into the core's
  `RETRO_MEMORY_SAVE_RAM` at load, writes it back whenever it changes (checked every 10 s), and
  writes it again on exit. The names and format match RetroArch's.
- **Save states** are in `states/<game>/slotN.state`, each with a `slotN.png` screenshot of the
  last frame. Slot 0 is the auto state, written whenever a session ends, so the player can
  continue later.

Every write goes to a temporary file and is renamed into place. The server never reads or writes
state data itself: it relays `save_state`/`load_state` to the session's worker, and only lists
and deletes the files.

### Web client and signalling

`SovEmu.Web` is Astro + Tailwind built to static files (no server runtime). The same Rust
process that serves `/api/*` serves those files, so the page and the API share one origin. That
process is `sovemu-server`; `sovemu-worker serve` does the same standalone for development.

Signalling is a single request. The browser gathers all its ICE candidates, then `POST`s the
complete offer (`{"type":"offer","sdp":...}`) to `/api/sessions/{id}/offer`, which the server
relays to that session's worker. The answer carries the server's
host candidates. The worker is ICE-lite and uses one UDP port for everything in its session. The client offers receive-only video and audio plus the `input` DataChannel
(unordered, no retransmits).

### Deployment: one Alpine container

SovEmu ships as a single self-contained image, Alpine (musl), with Debian as the fallback
only if a needed core cannot be made to work on musl. Everything runs in that one container:
the server, the per-session worker processes it spawns, and the cores. `tini` is PID 1, so
exited workers are reaped.

- **Cores are built from source** at commits pinned in `scripts/cores.list`, because the
  libretro buildbot's Linux cores are glibc builds and will not load on musl. Bumping a core
  means changing a hash in a reviewed PR.
- **The worker links musl dynamically** (`-C target-feature=-crt-static`). A statically
  linked binary cannot `dlopen` a core.
- **Every shipped core is probed at image build time** (`sovemu-worker probe`), which writes
  `/opt/sovemu/cores/manifest.json`. The server will read it to map file extensions to cores.
- The image runs as uid 1000 and keeps everything mutable under the `/data` volume: `roms/`,
  `system/` (BIOS), `saves/`, and later the SQLite database.
- Phase 1 adds `libvpx` and `opus` to the runtime image, both packaged by Alpine.

### Draft schema

- `users` (id, username, password_hash, is_admin, created_at)
- `identities` (user_id, provider, subject) for OIDC
- `auth_sessions` (refresh tokens)
- `invites`
- `systems`, `cores` (name, system, path, version)
- `roms` (id, system, title, sha1, file_path, size)
- `save_slots` (user, rom, slot, kind = sram | state, file_path, screenshot_path, updated_at)
- `play_sessions` (id, rom, core, user, worker_pid, status, started_at, ended_at)
- `session_players` (session, user, port, role); kept now so multiplayer can be added later
- `input_profiles` (user, rom nullable, device_type, mapping_json)
- `cheats` (rom, user, code, enabled)
- `ra_credentials` (user, encrypted token)

### Latency target

The target is under ~80 ms from button press to photon on a good network. The largest variable costs are frame alignment (up to 16.7 ms), encoding (1–3 ms at native resolution), the client's jitter buffer (reduce it with `playoutDelayHint` on the web and the equivalent native settings on mobile), and the network round trip. Libretro run-ahead is an option later.

## Open questions

- Which libretro core should be the reference for each system? Proposed: fceumm or Mesen (NES), snes9x (SNES), mGBA (GB/GBA), genesis_plus_gx (Genesis).
- How should workers communicate with the server over IPC: framed messages over a Unix socket, or over the worker's stdin/stdout?
- What sandboxing should workers get: systemd-run scopes, seccomp, or neither?

