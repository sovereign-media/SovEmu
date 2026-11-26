# SovEmu architecture

The v1 design and the decisions behind it, agreed in sovereign-media/SovEmu#1 (2026-11-13). The phases are in [ROADMAP.md](ROADMAP.md).

## Decisions

| Area | Decision |
|---|---|
| Systems | 2D cores (NES, SNES, GB/GBC/GBA, Virtual Boy, Mega Drive, Master System, Game Gear, SG-1000, Mega-CD, 32X, PC Engine and its CD, Atari 2600/7800/Lynx, Neo Geo Pocket, WonderSwan, Intellivision, ColecoVision, Vectrex, PS1, arcade, Neo Geo; a system is shown only once it has games) and, since the user asked for them, 3D cores that render with OpenGL: N64 (Mupen64Plus-Next), Dreamcast (Flycast), GameCube (Dolphin) and PS2 (PCSX2). |
| Server | Linux. A GPU is optional: the worker renders 3D cores through headless EGL on whichever GPU passes a startup probe (AMD/Intel via Mesa), else on Mesa's software renderer, llvmpipe (playable for lighter games, slow for GameCube and PS2). Video encoding stays on the CPU. |
| Scale | Personal/family, single node |
| Database | SQLite (WAL mode) via `sqlx` with migrations; large files stay on disk |
| Licensing | Non-commercial, so non-commercial cores (snes9x, genesis_plus_gx, etc.) are allowed |
| ROMs / BIOS | Shared library managed by the admin: the server scans a `roms/` folder; BIOS files are in `SovEmu__Paths__Bios` in any layout (a folder per system recommended), found by MD5 or by name and linked per session where each core reads them. No user uploads. |
| Multiplayer | Single-player first. The data model allows multiple players and spectators later. |
| Mobile clients | Native Kotlin (Android) and Swift (iOS), both using libwebrtc |
| Auth | Local accounts (argon2) with a role (`viewer` or `admin`). Sign-in is `SovEmu.Identity`: an OAuth 2.1 authorization server from the `oauth-as` crate, stored in SQLite, with OpenID Connect over it. The web client signs in with the code flow and PKCE; the apps with the device grant, or a password fallback; the API takes bearer access tokens |
| Video | VP8 (libvpx realtime, on the CPU's threads) or H.264, chosen for each client as it connects (`SovEmu__Streaming__Video__Codec=auto`, `codec.rs`): the cheapest to encode here and decode there; encoded at the console's native resolution. H.264 goes through libavcodec on the GPU's encoder (VA-API, NVENC) where one opens and returns every frame at once, else x264 (`superfast` + `zerolatency`): Constrained Baseline, no B-frames, keyframes on demand. A TV's hardware H.264 decoder can be far slower than its software VP8 one (23 ms against 3.6 ms a frame at 640×448 on an Amlogic TV), which is why a device's measured decode time replaces what it declares |
| Audio | Opus at 48 kHz, resampled from the core's own rate with `rubato`, sent as redundant audio (RED, RFC 2198) to clients that offer it, so a lost packet is rebuilt without a retransmission |
| Input | Unreliable, unordered WebRTC DataChannel; each packet carries a sequence number and the full button state |
| Designed for, not in v1 | Rewind/fast-forward, cheats, RetroAchievements (rcheevos), per-game controller remapping, client-side shaders |

## Architecture

**Constraint:** libretro cores keep their state in global variables, and their callbacks are bare C function pointers with no user-data pointer. Two instances of the same core can't share a process, and a crashing core takes its process down with it. So SovEmu runs **one worker process per game session**, managed by a supervisor server.

```
 Web / Android / iOS client
        │  HTTPS + WebSocket (auth, library, signaling)
        ▼
 ┌──────────────────────── sovemu-server (axum/tokio) ────────────────────────┐
 │ GraphQL API · auth · ROM library · SQLite · session orchestrator · signal │
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
- NAT traversal: clients get free public STUN servers by default (Cloudflare's and Google's), and a TURN relay built into the server (`SovEmu.Relay`, off unless `SovEmu__Relay__Enabled`) reaches the players no direct path does. It mints each signed-in player's short-lived credentials itself and relays only to the server's own game sessions, over loopback. An external TURN server (coturn) still works through `SovEmu__Streaming__IceServers`.

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
  SovEmu.Web/                Vite + React + TanStack Router (Radix UI), static; served by Rust [Phase 1]
  SovEmu.Android/            submodule: Kotlin + Compose, Stream libwebrtc, gamepad + touch
  SovEmu.iOS/                submodule: SwiftUI + XcodeGen, stasel/WebRTC, GameController + touch
  docs/  scripts/  .claude/  .github/
  cores/  roms/  system/     gitignored: fetched cores, admin ROM library, BIOS
```

Dependencies point one way. `Worker` uses `RetroHost` (which uses `LibretroFfi`), `Media` and
`Protocol`. `Media` knows nothing about libretro: it takes RGBA frames and interleaved samples.
`Protocol` is shared by `Worker` and `Server`. `Server` never links a core: it starts
`sovemu-worker` processes and talks to them over IPC.

### The API: GraphQL

The API is GraphQL (async-graphql), chosen to match Sovereign's and so that the web, Android and
iOS clients share one typed contract: `SovEmu.Server/schema.graphql`, checked in and kept current
by a test, from which the web client's TypeScript is generated. There is no REST API. Plain HTTP
remains only where GraphQL does not fit: files (`/media/…`, artwork and save screenshots, whose
URLs the API returns), and `/health` for probes. File URLs are signed as Sovereign signs its own:
an HMAC-SHA256 over the user, the path and an hour-rounded expiry, in `Sov-UserId`, `Sov-Expiry`
and `Sov-Signature`. Any client loads them without a cookie, a leaked one is good for one user's
files for a day, and they stay cacheable within the hour. Long operations
(starting a game, metadata lookups, mirror syncs) are followed by polling a query, not by
subscriptions.

### Server, sessions and the library

`sovemu-server` is the container's main process. It serves the web client and the GraphQL API
(`/graphql`; files the API points to under `/media/`), owns
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

### Library modes

`SovEmu__Library__Mode` sets how the server uses the library:
- **`direct`** (the default): read games from where they are. This is right for local
  disk or a NAS on the LAN.
- **`cache`**: for a remote share; see below.
- **`mirror`**: copy the library's games (optionally only some systems; never the artwork
  folders beside them) into `data/library` and play from there. A background sync copies new and changed files (size, and modification time
  within 2 s), removes deleted ones, and then rescans. The NAS stays mounted read-only as its
  source.

### Game information

`SovEmu.Library`'s `info/` holds one client per source (FBNeo DATs, ScreenScraper), each a
`Provider`: it turns a lookup into a `GameInfo` and, where the service has one, its own id for
the game. The server's `metadata/` asks the providers `SovEmu__Metadata__Sources` names, in that
order, and stores what they find; it names none of them, so a source can be replaced without
touching it. FBNeo's DATs name arcade and Neo Geo sets; ScreenScraper matches every other file
by MD5, size and name (hashes find hacks, translations and homebrew a name search misses), paces
its own requests, backs off a busy server and reports its quota with every answer. LaunchBox's
dump and TheGamesDB were sources once and were removed (migration `0030` cleared what they had
found). A background job, started by an admin or after a scan, tries the sources in order.
Earlier sources' fields win, and later ones fill the gaps. The result goes into `game_metadata`,
one row per game, found or not, so the next job skips it. Each row remembers which source filled
each field (`field_sources`) and each source's id for the game (`provider_ids`); a lookup merges
into the row, replacing only the fields it was asked to refresh and never clearing one a source
no longer knows. A lookup that failed is not stored, so it's retried next time. A source that
rejects credentials, runs out of quota or is closed is skipped for the rest of the job. A lookup stores pictures (box art, a screenshot, the logo) and a video only as URLs, with their source, never with
credentials (ScreenScraper's are stripped, and added again when downloading). When the lookup
ends, a second background job (`metadata/artwork.rs`) downloads them, `SovEmu__Artwork__MaxConcurrentDownloads`
at a time, giving up on a URL after three failures. Its queue is the table, so a restart resumes
it. The files live in `data/artwork/games/<game id>/` (`box.webp`, `shot.webp`, `marquee.webp`,
`video.mp4`), pictures as lossy WebP, and are a game's only
artwork: the scan reads ROMs alone, never the artwork folders a Batocera library keeps beside them
(`images/`, `videos/`; migration `0031` dropped what it had found there). The API's artwork
kinds map onto them: `thumb` is the box art, `image` the screenshot, `marquee` the logo. After
every scan a reconcile job (`metadata/reconcile.rs`) deletes what games no longer in the library
left (their information, downloaded files and resized copies) and forgets downloaded files that
went missing, so the download job fetches them again. The game API returns the looked-up title, with the file's own title in
`file_title`, plus description, release date, developer, publisher, genre, players and rating.

### A remote library

The library can sit on a share reached over a tunnel. That means about 100 ms per request and a
few MB/s. Three things keep the slowness away from playing:
- **Cartridge games:** a game is read into memory when it loads, so play itself never touches the
  network.
- **Disc games:** a disc is read during play on the core's own CD thread (PCSX-ReARMed, read-ahead
  256 sectors).
- **The cache:** `data/cache/games` holds what has been played:
  - a game is served from its copy once it has been played;
  - small files are copied straight away; disc images are copied after their first session;
  - least recently used files go first;
  - files that refer to other files (`.cue`, `.m3u`) stay on the share.

A copy counts as current when its size matches the original's and its modification time is within
2 s. Filesystems store these times at different precisions.

### Saves

Saves belong to a user and live under `/sov-data/saves/<user id>/<system>/`.
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

`SovEmu.Web` is a single-page app built with Vite to static files (no server runtime): one
`index.html`, whose screens TanStack Router picks from the URL without reloading the page. It
was Astro, one page per screen, until every page had become one React island; a page load per
screen then cost a fresh start of scripts, sign-in token and controllers for nothing. The screens
are built from Radix UI primitives (dialogs, selects, menus, progress, toasts) and Lucide icons;
the logic they drive (wire format, input, WebRTC, menu
stack, loading bar, home navigation) stays framework-free in `src/lib/`. The same Rust
process that serves `/graphql` serves those files (`index.html` for any page path), so the page
and the API share one origin. That
process is `sovemu-server`; `sovemu-worker serve` does the same standalone for development.

Signalling is a single request. The browser gathers all its ICE candidates, then sends the
complete offer's SDP with the `sessionOffer` mutation, which the server relays to that session's
worker and which returns the answer. The answer carries the server's
host candidates. The worker is ICE-lite and uses one UDP port for everything in its session. The client offers receive-only video and audio plus the `input` DataChannel
(unordered, no retransmits).

With the offer a client sends its **capabilities**: one complete, versioned document of what it
can decode and show (a row for each video codec: decoded in hardware or not, with a low-latency
mode or not, and the decoder's name; and its display), as Sovereign's clients do
(`SovEmu.Server/src/capabilities.rs`). The server refuses a document of another version with a
message asking the player to update the app, keeps a good one against the sign-in's device
(`sign_in_capabilities`, shown on `signIns`), and logs it with the session. Choosing each
session's codec from it is `codec.rs` (below); a client that sends no document is still served,
with VP8. The web client asks the browser's Media Capabilities about a WebRTC stream of
each codec (`src/lib/capabilities.ts`): `powerEfficient` is its word for hardware, and since a
browser names no decoder and has no low-latency switch, every row says low latency.

**The startup encoder test.** Before it listens, the server runs `sovemu-worker encoders` once
(`SovEmu.Sessions/src/encoders.rs`), as it runs `sovemu-worker gpu` for rendering: an encoder's
presence is not proof that it works. The worker runs each encoder a session could use (libvpx
VP8, H.264 on VA-API, on NVENC and on x264) exactly as a session would, threads included, on a
moving picture at 256×224, 640×448 and 1280×720, and reports whether it worked and the time a
frame took (colour conversion, any upload to the GPU, and the encode). The encoders run one
after another so they do not slow each other, the whole test has 10 seconds, split evenly, and
an encoder still running when its share is up counts as not working. No game can start before
it has finished. The result is logged (`encoder test`) and shown on the admin's Streaming page.

**Choosing a client's codec** (`SovEmu.Server/src/codec.rs`, `SovEmu__Streaming__Video__Codec=auto`). When a
client offers, each codec the server has a working encoder for and the client lists a decoder
for is priced in milliseconds a frame: the encoder test's time at the tested size nearest the
game's picture, plus the decode time. The decode time is what that device measured in its last
session with the codec (`device_decode_times`: `SovEmu.Sessions` averages the client's
once-a-second stats and tells the server every 20), and until there is one, a judgement from
what the device declares (4 ms for a hardware decoder with a low-latency mode, a frame more
without one, 6 ms at 640×448 in software). The cheapest wins; prices within a millisecond are a
tie, which goes to the codec encoded on the GPU, then to one decoded in hardware, then to VP8.
The server sends its choice to the worker with the offer (`Request::Offer`'s `codec`), which
answers with that codec alone. The prices are logged (`video codec chosen`) and kept on the
session (`codecChoice`). A measurement changes the codec at the device's next start, never in
the middle of a session.

**Keyframe requests** (`SovEmu.Worker/src/keyframes.rs`). A client that loses packets it cannot
get resent asks for a keyframe, and in a burst of loss it asks several times within a round trip.
Each keyframe is several times an ordinary frame, so the worker answers a burst with one, and
ignores a request that arrives before the last keyframe could have reached the client: within
the client's reported network round trip of it, at least 50 ms and at most 500 ms (100 ms until
the client has reported one). An answered request whose keyframe never comes is forgotten after
500 ms, so a client is never left asking. The `session health` and `session summary` lines count
keyframes, requests and ignored requests, and give the largest frame.

**Dropouts** (`keyframes.rs`, `adapt.rs`, `SovEmu__Streaming__Video__DropoutKeyframeMilliseconds`). A connected client is
never silent for long: its feedback on what it receives comes many times a second. One that has
been silent for 700 ms and is then heard again was cut off (Wi-Fi, mostly), and it has missed
that much video. Left alone, a browser waits on retransmissions for a second or more before it
asks for a keyframe, and the send-bandwidth estimator, having watched nothing arrive, paces what
follows at a crawl for many seconds. So the worker sends a keyframe at once, returns the bitrate
to what it was when the client was last heard, and starts the estimator again from its estimate
of that moment: a link that was down was not congested. Measured in Chromium with 100 ms of
delay, the picture came back 0.03 s after a 2-second dropout ended, where it had taken 1.3 s on
average and 1.8 s at worst.

**Packet priority** (`SovEmu.Worker/src/priority.rs`, `SovEmu__Streaming__Udp__Dscp`). Each session's socket
marks what it sends with a DSCP value, 34 (AF41, interactive video, as WebRTC marks a picture) by
default. A Wi-Fi access point maps that to its video queue, which is sent ahead of ordinary
traffic, so other use of the network delays a game less. Whatever sits between the server and the
access point may honour the mark, ignore it or clear it; 0 sends packets unmarked.

### Deployment: one Debian container

SovEmu ships as a single self-contained image on Debian (glibc), the same for every host.
(It began on Alpine; it moved because NVIDIA's driver, which most NVIDIA owners run and which
the NVIDIA container toolkit injects into the container, is built for glibc and cannot load on
musl. Mesa in the image still covers AMD, Intel and software rendering.) Everything runs in
that one container:
the server, the per-session worker processes it spawns, and the cores. `tini` is PID 1, so
exited workers are reaped.

- **Cores come prebuilt from a libretro stable release** (the bundle RetroArch ships for Linux
  x86_64), pinned by version and SHA-256 in `scripts/cores.list`; never nightly builds. This
  keeps SovEmu close to official releases without compiling emulators (they were built from
  source until the move to Debian, which musl required). Moving to a newer release is a
  reviewed change of version and hash. The cost: stable releases have no Linux arm64 cores, so
  the image is x86_64 only.
- **GPU drivers are found at run time:** libglvnd's libEGL dispatches to Mesa or to NVIDIA's
  driver, whichever the host provides; the image sets `NVIDIA_DRIVER_CAPABILITIES` so the
  toolkit injects graphics (and video, for NVENC later), not only compute.
- **Every shipped core is probed at image build time** (`sovemu-worker probe`), which writes
  `/opt/sovemu/cores/manifest.json`. The server will read it to map file extensions to cores.
- The image runs as uid 1000. What it ships is in `/opt/sovemu` (`bin/`, `cores/`, `web/`,
  `artwork/`), read-only; everything it writes is in the `/sov-data` volume (the database,
  `saves/`, `bios/`, `artwork/`, `cache/`), whose `artwork/` mirrors `/opt/sovemu/artwork`: a file
  there replaces the built-in one at the same path. The library is a read-only mount,
  `/sov-library`.
- The runtime image carries `libvpx`, `libopus` and FFmpeg's `libavcodec` (with x264) for
  encoding, and libva with Intel's (`intel-media-va-driver`, from Debian main: low-power encoding
  only, which is what SovEmu uses) and Mesa's VA-API drivers, from Debian.

### Draft schema

- `users` (id, username, password_hash, role, created_at)
- `identities` (user_id, provider, subject) for signing in with another provider, later
- `auth_sessions` (the browser's sign-in at the server's provider)
- `oauth_clients`, `oauth_authorization_codes`, `oauth_device_grants`, `oauth_access_tokens`,
  `oauth_refresh_tokens`, `oauth_revocation_barriers`: oauth-as's records (`SqliteStorage`)
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

The target is under ~80 ms from button press to photon on a good network. The largest variable costs are frame alignment (up to 16.7 ms), encoding (1–3 ms at native resolution), the client's jitter buffer (reduce it with `playoutDelayHint` on the web and the equivalent native settings on mobile), and the network round trip. Run-ahead (single-instance, per system, `SovEmu__Systems__<id>__RunAhead`) hides the games' own input lag on the 2D systems.

## Open questions

- Which libretro core should be the reference for each system? Proposed: fceumm or Mesen (NES), snes9x (SNES), mGBA (GB/GBA), genesis_plus_gx (Genesis).
- How should workers communicate with the server over IPC: framed messages over a Unix socket, or over the worker's stdin/stdout?
- What sandboxing should workers get: systemd-run scopes, seccomp, or neither?

