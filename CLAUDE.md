# SovEmu

A self-hosted console emulation server: a Rust backend runs libretro cores (through C bindings)
and streams them to web, Android and iOS clients over WebRTC. Personal/family scale, a single
Linux box (a GPU is optional: 3D systems render on it when present, else in software), SQLite
for storage. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) is the design and the decisions behind
it; [docs/ROADMAP.md](docs/ROADMAP.md) the phases; the tracking issue is sovereign-media/SovEmu#1.

The repository follows the layout of `sovereign-media/sovereign`: the backend lives in-tree under
`SovEmu.Backend/`, and each client is a project at the root. The native clients are their own
repositories, checked out as **unpinned submodules** (`branch = main`, `ignore = all`; update with
`git submodule update --remote`): `SovEmu.Android` (Kotlin + Compose, phones and Android TV) and
`SovEmu.iOS` (SwiftUI + XcodeGen, iPhone and iPad), and the documentation site, `SovEmu.Docs`
(Starlight, built and written the way Sovereign's documentation is). Each has its own README,
`.claude/rules/`, CI and PRs. A change here that changes what the docs say (a setting, a default,
a screen) gets a PR in `SovEmu.Docs` too. `SovEmu.Web` (Vite + React + TanStack Router, Tailwind) stays in-tree because the image
builds it from source.

## Rules that apply everywhere

The detail for each part loads from `.claude/rules/` when you work on its files (see below).

- **One libretro core per process, and the server never runs one.** The server starts one
  `sovemu-worker` per game session; cores and codecs are linked only into the worker.
- **Never lose a player's progress**: saves are written atomically, and a worker is always asked
  to shut down before it is stopped.
- **Never commit secrets, cores, ROMs or BIOS files.** Secrets come from the environment or
  BuildKit secrets; the ROM library is mounted read-only and never written.
- **The API is GraphQL, and only GraphQL**; files it hands out are signed URLs. A schema change,
  the input wire format and the controller definitions ([docs/CONTROLLERS.md](docs/CONTROLLERS.md))
  are shared by every client: change the server and all three clients together.
- **Every setting is an environment variable** (`SovEmu__<Area>__<Name>`); UI buttons are
  shortcuts, never the only way.
- **One image** (`SovEmu.Backend/Dockerfile`), Debian x86_64, the same for every host.
- **The logo comes from `brand/`.** `brand/generate.mjs` writes every client's icons, favicons,
  banner and brand mark from one drawing; change it and run it, never edit an output.
- **Long text goes through a file.** Commit messages and PR or issue bodies are written with the
  Write tool to `test-results/<task>.md` (named for the task) and passed with `-F` /
  `--body-file`, never a heredoc.
- **Trunk-based on `main`**: see [git-branching.md](.claude/rules/git-branching.md).

| Rule file | Loads for |
|---|---|
| `rust.md` | all of `SovEmu.Backend/`: crates, visibility, errors, unsafe, logging, tests |
| `backend-worker.md` | LibretroFfi, RetroHost, TestCore, Media, Worker: cores, GPU, saves, encoding |
| `backend-server.md` | Server, Sessions, Identity, Assets: GraphQL, sign-in, database, jobs, telemetry |
| `backend-library.md` | Library and the cores list: systems, providers, core releases |
| `settings.md` | `Config`, `.env.example`, compose: how a setting is added |
| `container.md` | Dockerfile, bake, compose, CI, hooks, scripts |
| `contracts.md` | schema, wire format, controllers, in every client |
| `web.md`, `react.md`, `web-tests.md` | `SovEmu.Web/`: architecture, React/TS style, tests |
| `test-roms.md` | the free ROMs, capture clips, the encoder bench and `docs/research/`: Artemio's 240p Test Suite first |

## Projects

All Rust crates live under `SovEmu.Backend/`, one Cargo workspace.

| Project | Crate | Purpose |
|---|---|---|
| `SovEmu.LibretroFfi` | `sovemu-libretro-ffi` | bindgen over `include/libretro.h` (types and constants) plus the C shim for libretro's variadic log callback. |
| `SovEmu.RetroHost` | `sovemu-retro-host` | The safe frontend: loads a core, answers the environment callback, runs a frame at a time; states, cheats, options, headless EGL. |
| `SovEmu.TestCore` | `sovemu-test-core` | A deterministic libretro core that tests load like a real one. Never shipped. |
| `SovEmu.Media` | `sovemu-media` | Encoding: colour conversion, H.264 (libavcodec), VP8 (libvpx), Opus. Knows nothing about libretro. |
| `SovEmu.Protocol` | `sovemu-protocol` | Wire formats: the input packets, signalling JSON, the worker IPC, the advertised addresses (`net.rs`). |
| `SovEmu.Relay` | `sovemu-relay` | The built-in TURN relay (over `turn-server-proto`): per-player minted credentials, relaying only to the server's own sessions. |
| `SovEmu.PortMapping` | `sovemu-port-mapping` | Asks the home router to forward the session and relay ports (UPnP IGD and NAT-PMP, both written here), renews and removes the mappings. |
| `SovEmu.Assets` | `sovemu-assets` | Signed file URLs and the server's generated secrets. |
| `SovEmu.Identity` | `sovemu-identity` | Accounts and sign-in: oauth-as with OpenID Connect over it. |
| `SovEmu.Library` | `sovemu-library` | The ROM library without the database: systems, scanning, labels, BIOS, metadata providers, cores. |
| `SovEmu.Worker` | `sovemu-worker` | The per-session binary (`session`, `serve`, `headless`, `probe`, `gpu`, `encoders`, `connection-test`). |
| `SovEmu.Sessions` | `sovemu-sessions` | Starts and drives workers; GPU and encoder reports; the connection test. Never links a core or a codec. |
| `SovEmu.Server` | `sovemu-server` | The process the container runs: axum, the GraphQL API, SQLite, sign-in, the web client's files. |
| `SovEmu.Web` | (npm `sovemu-web`) | The web client: a static Vite SPA the server serves. |

```
LibretroFfi  (root — no workspace references)        Media     (no workspace references)
    ├── TestCore   (types only; default features off) Protocol  (no workspace references)
    └── RetroHost
            └── Worker ──► Media, Protocol
Library   (no workspace references)                  Assets    (no workspace references)
Sessions ──► Library, Protocol   (starts workers; never links a core or a codec)
Identity  (no workspace references)                  Relay     (no workspace references)
PortMapping (no workspace references)
Server ──► Library, Assets, Identity, Sessions, Protocol, Relay, PortMapping   (never links a core or a codec)
```

## Build and test

```bash
cd SovEmu.Backend
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace     # once: cargo install cargo-nextest --locked
../scripts/coverage.sh            # line coverage → ../coverage/

cd ../SovEmu.Web && npm run lint && npm test && npm run build   # npm run e2e for the browser
```

The pre-push hook (`.githooks/pre-push`; once per clone, `git config core.hooksPath .githooks`)
runs these for whatever a push changes.

## Running locally (macOS)

Needs `brew install libvpx opus ffmpeg pkgconf` and Node 22.23.4+. From the repository root:

```bash
scripts/fetch-cores.sh fceumm                         # → cores/fceumm_libretro.dylib
scripts/fetch-free-roms.sh                            # → data/roms/{nes,snes,gb,gbc,gba,…}
(cd SovEmu.Web && npm ci && npm run build)            # → SovEmu.Web/dist
cd SovEmu.Backend && cargo build --release -p sovemu-worker -p sovemu-server
target/release/sovemu-server --data-dir ../data --cores-dir ../cores \
  --web-dir ../SovEmu.Web/dist --http 127.0.0.1:8080
# open http://127.0.0.1:8080: first visit creates the admin account
```

`npm run dev` in `SovEmu.Web` proxies the API to that server. `sovemu-worker headless` runs a game
with no server (`--core`, `--rom`, `--frames`, `--hold 200-205:start`, `--out`).
