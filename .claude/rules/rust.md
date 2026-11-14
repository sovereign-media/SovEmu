---
paths:
  - "SovEmu.Backend/**"
---

# Rust (SovEmu.Backend)

- **Crate layout.** One directory per crate, named `SovEmu.<Name>/`, with the package named
  `sovemu-<name>` in kebab case. Every crate inherits `version`, `edition`, `rust-version`,
  `repository` and `publish` from `[workspace.package]` and has `[lints] workspace = true`.
- **Dependency direction** is the graph in `CLAUDE.md`. `LibretroFfi` depends on nothing in the
  workspace, and nothing but `RetroHost` (and `TestCore`, for types) touches it: higher crates use
  the safe API. New crates join the graph as documented there; update the graph in the same PR.
- **unsafe.** Only `LibretroFfi`, `RetroHost` and `TestCore` contain `unsafe`. Every `unsafe`
  block gets a `// SAFETY:` comment saying why it holds; every `unsafe fn` gets a `# Safety`
  section. `unsafe_op_in_unsafe_fn` is denied.
- **FFI callbacks** are `unsafe extern "C" fn`s in `RetroHost/src/callbacks.rs`. They read and
  write `HostState` through `callbacks::with`, briefly, and never call into the core while
  holding it. A panic inside an `extern "C"` function aborts the process: acceptable in a
  per-session worker, but do not rely on it; return an error value where the API has one.
- **Environment commands.** Unsupported commands return `false` (logged once at debug). Adding
  one means: handle it in `HostState::environment`, cover it in `SovEmu.TestCore` if the test
  core can exercise it, and test it in `RetroHost/tests/test_core.rs`.
- **Errors.** Library crates return their own error enum (`std::error::Error`, no `anyhow`);
  binaries may use `anyhow` with `.context(...)`.
- **Logging** goes through `tracing`. Core log lines arrive under the `core` target.
- **Tests.** Unit tests in `#[cfg(test)]` modules; integration tests under `tests/`. Anything
  that loads a core takes the test file's `CORE_LOCK` first. Never make a test depend on a
  downloaded core or ROM.
- Before a PR: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, all from `SovEmu.Backend/`.
