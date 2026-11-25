---
paths:
  - "SovEmu.Backend/SovEmu.Library/**"
  - "scripts/cores.list"
  - "scripts/install-cores.sh"
  - "scripts/fetch-cores.sh"
---

# The ROM library and the cores (SovEmu.Library)

- **The library without the database.** `SovEmu.Library` reads files and asks services (the
  systems table, walking a library into games, release labels, BIOS files, the game-information
  providers behind one `Provider` interface, the cache and mirror, decorations, artwork layout).
  It never depends on `sqlx` or the server: work that needs the database stays in the server,
  calling the library's plain functions. Only what the server uses is `pub`.
- **SovEmu never writes to a ROM library**: the library volume is mounted read-only, locally too.
  Everything SovEmu writes goes under the data folder.
- **Shipped cores come from a libretro stable release**, never nightly. `scripts/cores.list` pins
  the release version and the SHA-256 of its `RetroArch_cores.7z`; `scripts/install-cores.sh`
  installs the listed cores plus data folders from git at pinned commits. x86_64 only (stable
  releases have no Linux arm64 cores).
- **Moving to a new release** also moves `FBNEO_COMMIT` (`src/info/fbneo.rs`) and
  `LIBRETRO_RELEASE`, and rechecks every workaround in `worker_env` (`src/cores.rs`: per-core
  environment for bugs in the release's builds, such as Mupen64Plus-Next's black picture on
  Mesa).
- **A new core needs its licence** in `src/cores.rs`; a test checks that table against
  `cores.list`.
- **Artwork layout** is `layout`: the built-in artwork in `/opt/sovemu/artwork` is laid out
  exactly as `/sov-data/artwork`, where a file at the same path replaces the built-in one. A new
  kind of image gets a folder in both. Never ship third-party bezel art.
- **ScreenScraper's developer credentials** are built in at compile time (see
  [container.md](container.md)); a user's own credentials are a setting. Neither is ever in the
  tree.
