# SovEmu

A self-hosted console emulation server: a Rust backend runs libretro cores (through C bindings)
and streams them to web, Android and iOS clients over WebRTC. Personal/family scale, a single
CPU-only Linux box, SQLite for storage. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) is the
design and the decisions behind it; [docs/ROADMAP.md](docs/ROADMAP.md) the phases. The tracking
issue is sovereign-media/SovEmu#1.

The repository follows the layout of `sovereign-media/sovereign`: the backend lives in-tree
under `SovEmu.Backend/`, and each client is a separate project at the root. `SovEmu.Web`
(Astro + Tailwind) is the first client and lives in-tree for now; it is self-contained so it can
become its own repository checked out as a submodule. `SovEmu.Android` and `SovEmu.iOS` will be
submodules from the start.

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
- **Never commit cores, ROMs or BIOS files.** `cores/`, `roms/`, `system/` and `data/` are
  gitignored; `scripts/fetch-cores.sh` downloads cores for local testing. Tests use
  `SovEmu.TestCore`.
- **The deliverable is one self-contained Alpine (musl) image**, `SovEmu.Backend/Dockerfile`.
  Never ship a buildbot core in it: those are glibc builds and do not load on musl. Shipped
  cores are built from source at the commits pinned in `scripts/cores.list`. Never build the
  worker with musl's default static linking (`crt-static`): a static binary cannot `dlopen`.
  Debian is the fallback only if a needed core cannot be made to work on musl.
- **The server never runs a core.** Cores and codecs live only in `sovemu-worker`; the server
  starts one `sovemu-worker session` per game and talks to it over stdin/stdout. Never link
  `RetroHost` or `Media` into `SovEmu.Server`.
- **Never write a migration's SQL into Rust.** Schema changes are a new numbered file in
  `SovEmu.Server/migrations/`; never edit one that has shipped.
- **The web client is served by the Rust process that serves the API**: `SovEmu.Web` builds to
  static files (`output: "static"`, no Astro server runtime, no Node in the image), and the Rust
  side serves them next to `/api/*`. Never add a Node server or an SSR adapter.
- **The input wire format exists twice**, in `SovEmu.Protocol/src/input.rs` and
  `SovEmu.Web/src/lib/protocol.ts`, each tested against the same byte vectors. Change both in
  the same PR, and bump `INPUT_VERSION` if an old client would misread the new format.
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
| `SovEmu.Worker` | `sovemu-worker` | The per-session binary. `session` is one game driven by the server over JSON lines on stdin/stdout (`SovEmu.Protocol::ipc`), streaming over WebRTC (str0m, one UDP port per session); `serve` is the same loop standalone with its own HTTP (development, tests); `headless` dumps PNG frames, a WAV and a timing summary; `probe` loads cores and reports their info as JSON. |
| `SovEmu.Server` | `sovemu-server` | The process the container runs: axum serving `SovEmu.Web` and `/api/*`; SQLite (sqlx, WAL, embedded migrations in `migrations/`); local accounts (argon2, HttpOnly cookie, first-run admin setup); the systems table and library scanner (Recalbox folder names + aliases, extension fallback); one `sovemu-worker session` per game session with a UDP port from a pool. |
| `SovEmu.Web` | (npm `sovemu-web`) | The web client: Astro + Tailwind v4, built to static files the Rust server serves. Vitest for unit tests; `astro check` in the build. |

```
LibretroFfi  (root — no workspace references)        Media     (no workspace references)
    ├── TestCore   (types only; default features off) Protocol  (no workspace references)
    └── RetroHost
            └── Worker ──► Media, Protocol
Server ──► Protocol   (never links a core or a codec: it runs workers)
```

Tests sit beside the code they test (`#[cfg(test)]` modules) and in each crate's `tests/`
directory; there are no separate test crates. Integration tests build `sovemu-test-core` on
first use (or take `SOVEMU_TEST_CORE`) and serialise on a lock, since only one core may be
loaded per process.

## Container

`SovEmu.Backend/Dockerfile` (context: the repository root) has five stages: `web` runs the web
client's tests and builds it on `node:<ver>-alpine`; `cores` builds every core in
`scripts/cores.list` with `scripts/build-cores.sh`; `build` compiles the workspace on
`rust:<ver>-alpine` with `RUSTFLAGS=-C target-feature=-crt-static` against Alpine's libvpx and
opus; `test` runs `cargo test --workspace` on musl; `runtime` is plain Alpine with `libgcc`,
`libstdc++`, `libvpx`, `opus` and `tini`, the web client in `/opt/sovemu/web`, runs as uid 1000,
keeps data in `/data`, exposes 8080/tcp (web + API) and 50000/udp (all WebRTC media), and runs
`sovemu-worker probe` over every core at build time (writing `/opt/sovemu/cores/manifest.json`),
so a core that cannot load fails the build. `compose.yaml` builds it, mounts `./data` and runs
`serve`.

```bash
docker build -f SovEmu.Backend/Dockerfile --target test .   # tests on musl
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

For web development, `npm run dev` in `SovEmu.Web` proxies `/api` to `127.0.0.1:8080`
(override with `SOVEMU_API`), so run `serve` on that address alongside it.

## Conventions

- Rust 2024 edition; `rustfmt.toml` (100 columns) and workspace lints in `SovEmu.Backend/Cargo.toml`.
  Every `unsafe` block carries a `// SAFETY:` comment (clippy enforces it).
- Rules load from `.claude/rules/`: `git-branching.md` always, `rust.md` for backend code,
  `web.md` for `SovEmu.Web`.
- Trunk-based on `main`: branch from a freshly fetched `origin/main` as `feat/`, `fix/`,
  `chore/` or `docs/`, and PR back to `main`.
