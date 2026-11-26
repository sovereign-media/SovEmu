---
paths:
  - "SovEmu.Backend/**"
---

# Rust (SovEmu.Backend)

How every crate is written. What each crate may do is in the files for its part:
[backend-worker.md](backend-worker.md) (cores, encoding, the worker),
[backend-server.md](backend-server.md) (server, sessions, identity, assets),
[backend-library.md](backend-library.md), [settings.md](settings.md) and
[container.md](container.md).

## Crates

- **Rust 2024 edition**, `rust-version` the image's own toolchain (`RUST_VERSION` in the
  Dockerfile): the one version anything is built and tested with, so the two move together.
  `rustfmt.toml` sets 100 columns.
- **One directory per crate**, `SovEmu.<Name>/`, its package `sovemu-<name>`. Every crate inherits
  `version`, `edition`, `rust-version`, `repository`, `license` (AGPL-3.0-or-later) and `publish`
  from `[workspace.package]` and has `[lints] workspace = true`.
- **The dependency direction is the graph in `CLAUDE.md`.** A new crate, or a new edge, joins the
  graph there in the same PR. Neither `SovEmu.Server` nor `SovEmu.Sessions` ever links
  `RetroHost` or `Media`.
- **Visibility**: `unreachable_pub` is on. An item is `pub` only if another crate uses it;
  `pub(crate)` inside one (a binary's modules included). A library crate's public items are its
  interface: keep it small.
- **Errors**: library crates return their own error enum implementing `std::error::Error`, never
  `anyhow` (a few older functions return `String` or `io::Error`; give them an enum when you
  change them). Binaries (`sovemu-server`, `sovemu-worker`) use `anyhow` with `.context(...)`.
- Every type implements `Debug` (`missing_debug_implementations`).

## unsafe

- `unsafe` lives in `LibretroFfi`, `RetroHost`, `Media` and `TestCore`, plus the server's
  `std::env::set_var` / `remove_var` when it applies environment changes (`main.rs`). Nowhere else.
- Every `unsafe` block has a `// SAFETY:` comment saying why it holds; every `unsafe fn` has a
  `# Safety` section. `unsafe_op_in_unsafe_fn` is denied, and clippy's
  `undocumented_unsafe_blocks` fails the build under `-D warnings`.

## Logging

Through `tracing`, **with fields, not formatted strings**:
`tracing::info!(game = %title, "session started")`, not `info!("session {id} started")`, since
`SovEmu__Logging__Format=json` turns fields into keys. Work for a request or a session runs inside
its span (`request`, `session`), so its lines carry the route or the session id; a worker gets its
session id as `--session-id`. Core log lines arrive under the `core` target.

## Tests

- Unit tests in `#[cfg(test)]` modules beside the code; integration tests in each crate's
  `tests/` (the server's HTTP tests are the in-crate `http_tests.rs`). No separate test crates.
- **cargo-nextest** runs them (`.config/nextest.toml`; the container uses `--profile ci`): every
  test is its own process, all at once. A test must not rely on another test's process state.
  Tests in a file that loads a core in-process take its `CORE_LOCK` first (needed under
  `cargo test`).
- Tests run beside every other test on a loaded CI machine: **wait for what you measure** (a count
  of frames or acks), never for a fixed time alone.
- Never depend on a downloaded core or ROM: tests use `SovEmu.TestCore`, built on first use (or
  `SOVEMU_TEST_CORE`).
- Coverage is `../scripts/coverage.sh` (cargo-llvm-cov in its own target folder), never bare
  `cargo llvm-cov`. It reports, never gates; a new module with little coverage is a test to write.

## Before a PR

From `SovEmu.Backend/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D
warnings`, `cargo nextest run --workspace`. The pre-push hook runs them for any push that touches
the backend; never skip it with `--no-verify` to get a failing change up.
