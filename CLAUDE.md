# SovEmu

A self-hosted console emulation server: a Rust backend runs libretro cores (through C bindings)
and streams them to web, Android and iOS clients over WebRTC. Personal/family scale, a single
Linux box (a GPU is optional: 3D systems render on it when present, else in software), SQLite
for storage. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) is the
design and the decisions behind it; [docs/ROADMAP.md](docs/ROADMAP.md) the phases. The tracking
issue is sovereign-media/SovEmu#1.

The repository follows the layout of `sovereign-media/sovereign`: the backend lives in-tree
under `SovEmu.Backend/`, and each client is a separate project at the root. The native clients are
their own repositories, checked out as **unpinned submodules** (`branch = main`, `ignore = all`;
update with `git submodule update --remote`): `SovEmu.Android` (Kotlin + Compose) and
`SovEmu.iOS` (SwiftUI + XcodeGen). Each has its own README, `.claude/rules/`, CI and PRs.
`SovEmu.Web` (Astro + Tailwind, React islands) stays in-tree for now because the image builds it from source;
it is self-contained so it can move to its own repository once the image takes a released build
instead (as Sovereign's server takes its SPA).

## Critical rules

- **One libretro core per process.** Cores keep their state in globals and their callbacks carry
  no user data, so `sovemu-retro-host` keeps process-global callback state and `Core::load`
  refuses a second core. Never try to run two sessions in one process; the server spawns one
  `sovemu-worker` per session. The same isolation means a crashing core only kills its session.
- **Never hold the callback-state lock across a call into the core.** The core calls back into
  us from inside `retro_run`, `retro_load_game` and `retro_set_environment`; holding the lock
  there deadlocks. `callbacks::with` is for short reads and writes only.
- **Never edit `SovEmu.LibretroFfi/include/libretro.h`** or the generated bindings. Update the
  header from upstream and record the commit in `include/README.md`.
- **Never commit secrets.** NAS credentials live in `.env` (gitignored and dockerignored; see
  `.env.example`), read by `compose.nas.yaml`. The NAS library is mounted read-only; SovEmu never
  writes to a ROM library.
- **Never commit cores, ROMs or BIOS files.** `cores/`, `roms/`, `system/` and `data/` are
  gitignored; `scripts/fetch-cores.sh` downloads cores for local testing. Tests use
  `SovEmu.TestCore`.
- **The deliverable is one self-contained Debian (glibc) x86_64 image**,
  `SovEmu.Backend/Dockerfile`, the same for every host: Mesa in the image drives AMD, Intel and
  software rendering, and the NVIDIA container toolkit injects NVIDIA's own (glibc) driver when
  there is one. Never split it into per-GPU images.
- **Shipped cores come from a libretro stable release**, never nightly: `scripts/cores.list`
  pins the release version and the SHA-256 of its `RetroArch_cores.7z`, and
  `scripts/install-cores.sh` installs the listed cores plus data folders from git at
  pinned commits. x86_64 only, because stable releases have no Linux arm64 cores. Moving to a
  new release also moves `FBNEO_COMMIT` (`SovEmu.Library/src/info/fbneo.rs`).
- **The server never runs a core.** Cores and codecs live only in `sovemu-worker`; the server
  starts one `sovemu-worker session` per game and talks to it over stdin/stdout. Never link
  `RetroHost` or `Media` into `SovEmu.Server`.
- **Never lose a player's progress.** Battery saves and states are written atomically (temp file
  + rename) by the worker, which also writes them on every exit path (`Session::finish`, called
  from `Drop`). Never write a save in place, and never kill a worker without first asking it to
  shut down (`Workers::stop` does both, in that order).
- **Never write a migration's SQL into Rust.** Schema changes are a new numbered file in
  `SovEmu.Server/migrations/`; never edit one that has shipped.
- **The web client is served by the Rust process that serves the API**: `SovEmu.Web` builds to
  static files (`output: "static"`, no Astro server runtime, no Node in the image), and the Rust
  side serves them next to `/graphql`. Never add a Node server or an SSR adapter.
- **The API is GraphQL, and only GraphQL** (`SovEmu.Server/src/graphql.rs`, async-graphql):
  `POST /graphql` for every client (web, Android, iOS), GraphiQL at `GET /graphql`. Never add a
  REST route. Plain HTTP is only for files the API hands out URLs to (`/media/…`: artwork,
  videos, save screenshots, decorations) and for `/health`. Those URLs are always **signed** for
  the requesting user (`state.assets.sign`, `SovEmu.Assets`, Sovereign's scheme); a `/media/` route
  sits behind `require_signature` and serves the signed user's data, never a cookie's. Never hand
  out or serve an unsigned file URL. Resolvers check access
  (`user(ctx)`, `admin(ctx)`) and call plain functions in the other modules; errors carry
  `extensions.code`. `Int` is 32-bit, so byte counts and Unix times are `Float`. The schema is
  checked in as `SovEmu.Server/schema.graphql` (a test fails when it drifts:
  `UPDATE_SCHEMA=1 cargo test -p sovemu-server schema_file`), and the web client's typed
  documents are generated from it (`npm run codegen` in `SovEmu.Web`, committed in
  `src/graphql/`). A schema change updates all three clients in the same change.
- **The input wire format exists twice**, in `SovEmu.Protocol/src/input.rs` and
  `SovEmu.Web/src/lib/protocol.ts` (input `0x01`, hotkeys `0x02`, analog `0x03`, ack `0x81`),
  each tested against the same byte vectors. Change both in
  the same PR, and bump `INPUT_VERSION` if an old client would misread the new format.
- **Controllers are described once, in [docs/CONTROLLERS.md](docs/CONTROLLERS.md)**: their IDs
  (known models such as `xbox` and `dualsense`, else `pad:<vendor>:<product>`), the positional
  input names, labels, defaults, hotkeys and players. The web client
  (`SovEmu.Web/src/lib/controllers.ts`), Android and iOS each implement it, and the server
  (`controllers.rs`) stores each user's mappings per controller ID and checks them against it.
  Change the document and all three clients together.
- **Every setting is an environment variable; nothing needs click-ops.** A new setting gets a
  `SOVEMU_*` env var on `Config` (and a row in the README's table and a line in
  `.env.example`). Secrets join `SECRETS` in `main.rs` (so `<VAR>_FILE` works and the startup
  log redacts them). Anything an admin can do in the UI must also happen on its own from
  configuration (accounts, rescans, mirror syncs, metadata lookups, per-system cores and
  options); UI buttons are shortcuts, never the only way. Where the UI stores a setting
  (emulator settings, `system_settings`), the environment still wins, the UI shows it locked,
  and the page offers the stored values as environment lines. Tunables are settings, not constants.
  Lists and maps use Sovereign's `__` hierarchy (`SOVEMU_USERS__0__USERNAME`,
  `SOVEMU_SYSTEMS__psx__OPTIONS__<name>`, parsed in `structured.rs`). A new setting also gets
  its place in `charts/sovemu` if it needs more than `settings:` (a secret, a volume, a port).
- **GPU rendering is headless EGL in the worker** (`SovEmu.RetroHost/src/gl.rs`): libEGL is
  opened at run time, never linked, so the worker still runs without Mesa. A GPU core's frames
  are read back from a framebuffer object after `retro_run`; the context is current on the
  thread that runs the core and must be made current before any call into it. The server only
  ever learns about GPUs from `sovemu-worker gpu` (it never loads GL); a device counts only
  if it drew a frame and read it back, and `SOVEMU_GPU` falls back to software (llvmpipe), then
  to no 3D systems. A GPU with no DRM node (NVIDIA without `nvidia-drm`) is reported as
  `auto`, which workers resolve hardware-first. Vulkan cores are not supported yet.
- **Workers get no secrets.** `Workers::spawn` clears the environment (`WORKER_ENV` is the
  allow-list); pass what a worker needs as arguments.
- **Never expose operational data without a switch.** Telemetry is pushed, never served: traces
  and metrics go out over OpenTelemetry (OTLP/HTTP, `telemetry.rs`) only when
  `SOVEMU_OTEL_ENDPOINT` is set, and there is no metrics route. New metrics are instruments in
  `telemetry.rs`; new operational routes stay off unless a setting turns them on.
- **Log with fields, not formatted strings.** `tracing::info!(game = %title, "session started")`,
  not `info!("session {id} started")`: `SOVEMU_LOG_FORMAT=json` turns fields into keys. Work for
  a request or a session runs inside its span (`request`, `session`), so its lines carry the
  route or the session id; a worker gets its session id as `--session-id`.
- **The server shuts down on SIGTERM as on Ctrl-C**: it stops every worker (so saves are
  written) before exiting. Kubernetes and `docker stop` send SIGTERM; never exit without it.
- **Long text goes through a file.** Commit messages, PR and issue bodies are written with the
  Write tool to `test-results/<task>.md` and passed with `-F` / `--body-file`, never a heredoc.

## Projects

All Rust crates live under `SovEmu.Backend/`, one Cargo workspace. Build and test from there:

```bash
cd SovEmu.Backend
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

| Project | Crate | Purpose |
|---|---|---|
| `SovEmu.LibretroFfi` | `sovemu-libretro-ffi` | bindgen over the copy of `libretro.h` in `include/`: types and constants only, plus the C shim that adapts libretro's variadic log callback. Root of the dependency graph. |
| `SovEmu.RetroHost` | `sovemu-retro-host` | The safe frontend: `Core` loads a core with libloading, answers the environment callback, runs a frame at a time, and returns its video, audio and timing changes. Save states, cheats, core options. |
| `SovEmu.TestCore` | `sovemu-test-core` | A deterministic libretro core (cdylib) that tests load with dlopen exactly like a real one. Never shipped. |
| `SovEmu.Media` | `sovemu-media` | Encoding: RGBA→I420, VP8 (system libvpx) tuned for real time, Opus (system libopus, restricted low delay), and `AudioEncoder` (resample any core rate to 48 kHz, 10 ms packets). bindgen over libvpx/libopus via pkg-config. Knows nothing about libretro. |
| `SovEmu.Protocol` | `sovemu-protocol` | Wire formats every client implements: the binary input packet and its ack on the `input` DataChannel, and the JSON session descriptions used for signalling. |
| `SovEmu.Assets` | `sovemu-assets` | Signed file URLs, as Sovereign's: `AssetSigner` signs a path for a user (HMAC-SHA256, `Sov-UserId`/`Sov-Expiry`/`Sov-Signature`) and checks one; `load_key` (`SOVEMU_ASSET_KEY`, else generated into `data/server.json`). No HTTP, no database: the server's `/media/…` middleware calls it. |
| `SovEmu.Library` | `sovemu-library` | The ROM library without the database: the systems table (Recalbox folder names + aliases, extension fallback), walking a library into games (parts folded together, soft patches found, artwork file names), release labels, BIOS files, the game-information clients (FBNeo, LaunchBox, ScreenScraper, TheGamesDB), the local cache and mirror, decorations. Reads files and asks services; never opens the database. Only what the server uses is `pub`. |
| `SovEmu.Worker` | `sovemu-worker` | The per-session binary. `session` is one game driven by the server over JSON lines on stdin/stdout (`SovEmu.Protocol::ipc`), streaming over WebRTC (str0m, one UDP port per session); `serve` is the same loop standalone with its own HTTP (development, tests); `headless` dumps PNG frames, a WAV and a timing summary; `probe` loads cores and reports their info as JSON. |
| `SovEmu.Server` | `sovemu-server` | The process the container runs: axum serving `SovEmu.Web` and the GraphQL API at `/graphql`; SQLite (sqlx, WAL, embedded migrations in `migrations/`); local accounts (argon2, HttpOnly cookie, first-run admin setup); the library's tables, kept in step with what `SovEmu.Library` finds; one `sovemu-worker session` per game session with a UDP port from a pool. |
| `SovEmu.Web` | (npm `sovemu-web`) | The web client: Astro + Tailwind v4 with React islands on Radix UI primitives and Lucide icons, built to static files the Rust server serves. Vitest for unit tests; `astro check` in the build. |

```
LibretroFfi  (root — no workspace references)        Media     (no workspace references)
    ├── TestCore   (types only; default features off) Protocol  (no workspace references)
    └── RetroHost
            └── Worker ──► Media, Protocol
Library   (no workspace references)                  Assets    (no workspace references)
Server ──► Library, Assets, Protocol   (never links a core or a codec: it runs workers)

Worker modules: stream.rs (the loop), session.rs (stdin/stdout IPC), serve.rs (standalone HTTP),
persist.rs (battery saves and states), patch.rs (IPS/UPS/BPS soft patches), rewind.rs, headless.rs, probe.rs (and standin.rs, the
stand-in games that make option-on-load cores declare their options).
```

Tests sit beside the code they test (`#[cfg(test)]` modules) and in each crate's `tests/`
directory; there are no separate test crates. Integration tests build `sovemu-test-core` on
first use (or take `SOVEMU_TEST_CORE`) and serialise on a lock, since only one core may be
loaded per process.

## Container

`SovEmu.Backend/Dockerfile` (context: the repository root) has six stages: `web` runs the web
client's tests and builds it on `node:<ver>-trixie-slim`; `cores` installs every core in
`scripts/cores.list` from the libretro stable release with `scripts/install-cores.sh` (refusing
to build for anything but `linux/amd64`); `toolchain` is `rust:<ver>-trixie` with the sources
and Debian's libvpx and libopus; `build` compiles the release binaries and `test` runs
`cargo test --workspace` with Mesa's llvmpipe, both from `toolchain` and side by side, each with
its own `target/` cache mount (CI builds both at once with `SovEmu.Backend/docker-bake.hcl` and
keeps the cache mounts between runs); `runtime` is `debian:trixie-slim` with
`libstdc++6`, `libvpx9`, `libopus0`, `tini`, libglvnd and Mesa (GL, EGL; no Vulkan drivers yet),
`libxi6` and `libusb-1.0-0` (the release's Dolphin links them), sets
`NVIDIA_DRIVER_CAPABILITIES=graphics,video,utility` for NVIDIA's toolkit, has the web client in
`/opt/sovemu/web`, runs as uid 1000,
keeps data in `/data`, exposes 8080/tcp (web + API) and 50000/udp (all WebRTC media), and runs
`sovemu-worker probe` over every core at build time (writing `/opt/sovemu/cores/manifest.json`),
draws the built-in decoration set into `/opt/sovemu/decorations` (`sovemu-server
--write-decorations`; never ship third-party bezel art),
so a core that cannot load fails the build. `compose.yaml` builds it, mounts `./data` and runs
`serve`.

```bash
docker build --platform linux/amd64 -f SovEmu.Backend/Dockerfile --target test .   # tests
docker compose build && docker compose run --rm sovemu probe
scripts/fetch-free-roms.sh && docker compose up --build     # then open http://localhost:8080
```

The container runs `sovemu-server` on 8080; each game session's worker streams on its own UDP
port from 50000-50009 (`SOVEMU_UDP_PORTS`). In a container the server cannot see the host's
address, so `SOVEMU_ADVERTISE_IP` (default `127.0.0.1` in compose) must be the address browsers
reach the host on. `scripts/fetch-free-roms.sh` fills `data/roms` with freely licensed homebrew
until the NAS library is connected.

## Running locally (macOS)

Needs `brew install libvpx opus pkgconf` and Node 22.23.4+.

```bash
scripts/fetch-cores.sh fceumm                         # → cores/fceumm_libretro.dylib
(cd SovEmu.Web && npm ci && npm run build)            # → SovEmu.Web/dist
cd SovEmu.Backend && cargo build --release -p sovemu-worker
scripts/fetch-free-roms.sh                            # → data/roms/{nes,snes,gb,gbc,gba}
cargo build --release -p sovemu-worker -p sovemu-server
target/release/sovemu-server --data-dir ../data --cores-dir ../cores \
  --web-dir ../SovEmu.Web/dist --http 127.0.0.1:8080
# open http://127.0.0.1:8080: first visit creates the admin account

target/release/sovemu-worker headless --core ../cores/fceumm_libretro.dylib \
  --rom /path/to/game.nes --frames 600 --hold 200-205:start --out ../headless-out
```

For web development, `npm run dev` in `SovEmu.Web` proxies `/graphql` and `/media` to `127.0.0.1:8080`
(override with `SOVEMU_API`), so run `serve` on that address alongside it.

## Conventions

- Rust 2024 edition; `rustfmt.toml` (100 columns) and workspace lints in `SovEmu.Backend/Cargo.toml`.
  Every `unsafe` block carries a `// SAFETY:` comment (clippy enforces it).
- Rules load from `.claude/rules/`: `git-branching.md` always, `rust.md` for backend code,
  `web.md` for `SovEmu.Web`.
- Trunk-based on `main`: branch from a freshly fetched `origin/main` as `feat/`, `fix/`,
  `chore/` or `docs/`, and PR back to `main`.
