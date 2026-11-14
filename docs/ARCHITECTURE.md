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
`SovEmu.Backend/`, one directory per crate. Each client is a separate repository, checked out
at the root as an unpinned submodule, so the server/client boundary is also a git boundary.

```
SovEmu/
  SovEmu.Backend/            Cargo workspace (built and tested from here)
    SovEmu.LibretroFfi/      bindgen over include/libretro.h + variadic log shim     [Phase 0]
    SovEmu.RetroHost/        safe frontend: load core, environment, run loop         [Phase 0]
    SovEmu.TestCore/         deterministic cdylib core for tests                     [Phase 0]
    SovEmu.Worker/           per-session binary (headless now, WebRTC later)         [Phase 0]
    SovEmu.Media/            pixel conversion, Encoder trait (libvpx, x264), Opus    [Phase 1]
    SovEmu.Protocol/         shared serde types: IPC, input packets, signalling      [Phase 1]
    SovEmu.Server/           axum API, auth, SQLite, worker orchestration            [Phase 2]
  SovEmu.Web/                submodule: TypeScript, browser WebRTC + Gamepad API
  SovEmu.Android/            submodule: Kotlin, libwebrtc + InputDevice
  SovEmu.iOS/                submodule: Swift, WebRTC.framework + GameController
  docs/  scripts/  .claude/  .github/
  cores/  roms/  system/     gitignored: fetched cores, admin ROM library, BIOS
```

Dependencies point one way. `Worker` uses `Media`, which uses `RetroHost`, which uses
`LibretroFfi`. `Protocol` is shared by `Worker` and `Server`. `Server` never links a core: it
starts `sovemu-worker` processes and talks to them over IPC.

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

