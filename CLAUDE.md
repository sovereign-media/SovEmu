# SovEmu

A self-hosted console emulation server: a Rust backend runs libretro cores (through C bindings)
and streams them to web, Android and iOS clients over WebRTC. Personal/family scale, a single
CPU-only Linux box, SQLite for storage. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) is the
design and the decisions behind it; [docs/ROADMAP.md](docs/ROADMAP.md) the phases. The tracking
issue is sovereign-media/SovEmu#1.

The repository follows the layout of `sovereign-media/sovereign`: the backend lives in-tree
under `SovEmu.Backend/`, and each client will be its own repository checked out here as a
submodule (`SovEmu.Web`, `SovEmu.Android`, `SovEmu.iOS`). None exists yet.

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
| `SovEmu.Worker` | `sovemu-worker` | The per-session binary. Phase 0: `headless` dumps PNG frames, a WAV and a timing summary; `probe` loads cores and reports their name, version and extensions as JSON. Later: WebRTC session mode. |

Planned: `SovEmu.Media` (VP8/H.264 + Opus encoding), `SovEmu.Protocol` (shared IPC, input and
signalling types), `SovEmu.Server` (axum API, auth, SQLite, worker orchestration).

```
LibretroFfi  (root — no workspace references)
    ├── TestCore   (types only; default features off)
    └── RetroHost
            └── Worker
```

Tests sit beside the code they test (`#[cfg(test)]` modules) and in each crate's `tests/`
directory; there are no separate test crates. Integration tests build `sovemu-test-core` on
first use (or take `SOVEMU_TEST_CORE`) and serialise on a lock, since only one core may be
loaded per process.

## Container

`SovEmu.Backend/Dockerfile` (context: the repository root) has four stages: `cores` builds every
core in `scripts/cores.list` with `scripts/build-cores.sh`; `build` compiles the workspace on
`rust:<ver>-alpine` with `RUSTFLAGS=-C target-feature=-crt-static`; `test` runs
`cargo test --workspace` on musl; `runtime` is plain Alpine with `libgcc`, `libstdc++` and
`tini`, runs as uid 1000, keeps data in `/data`, and runs `sovemu-worker probe` over every
core at build time (writing `/opt/sovemu/cores/manifest.json`), so a core that cannot load
fails the build. `compose.yaml` builds it and mounts `./data`.

```bash
docker build -f SovEmu.Backend/Dockerfile --target test .   # tests on musl
docker compose build && docker compose run --rm sovemu probe
```

## Running a real core locally

```bash
scripts/fetch-cores.sh fceumm               # → cores/fceumm_libretro.{so,dylib}
cd SovEmu.Backend && cargo build --release -p sovemu-worker
target/release/sovemu-worker headless --core ../cores/fceumm_libretro.dylib \
  --rom /path/to/game.nes --frames 600 --hold 200-205:start --out ../headless-out
```

## Conventions

- Rust 2024 edition; `rustfmt.toml` (100 columns) and workspace lints in `SovEmu.Backend/Cargo.toml`.
  Every `unsafe` block carries a `// SAFETY:` comment (clippy enforces it).
- Rules load from `.claude/rules/`: `git-branching.md` always, `rust.md` for backend code.
- Trunk-based on `main`: branch from a freshly fetched `origin/main` as `feat/`, `fix/`,
  `chore/` or `docs/`, and PR back to `main`.
