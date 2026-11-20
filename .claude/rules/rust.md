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
- **Library.** `SovEmu.Library` is the ROM library without the database: it reads files and
  asks services, and never depends on `sqlx` or the server. Work that needs the database stays in
  the server, calling the library's plain functions. Only what the server uses is `pub`;
  everything else is `pub(crate)` or private, so its internals can change freely.
- **Visibility.** `unreachable_pub` is a workspace lint: an item is `pub` only if another crate
  uses it. A library crate's public items are its interface; keep it small.
- **Identity.** `SovEmu.Identity` is accounts and sign-in, on the `oauth-as` crate. Protocol
  logic (codes, PKCE, refresh rotation, revocation, the device grant) is oauth-as's: build around
  it, never beside it. `SqliteStorage` keeps passing oauth-as's conformance harness.
- **Server.** `SovEmu.Server` depends on `Library`, `Assets`, `Identity`, `Sessions` and `Protocol` only; neither it nor
  `Sessions` ever links `RetroHost` or `Media`. Database access is plain
  `sqlx::query` / `query_as` with bound parameters (never format values into SQL); schema changes are new
  files in `migrations/`. The API is GraphQL (`graphql.rs`): resolvers check access with
  `user(ctx)` / `admin(ctx)` and call plain functions (`&AppState`, `&User`, arguments) in the
  other modules; never add a REST route. After changing the schema, regenerate
  `schema.graphql` (`UPDATE_SCHEMA=1 cargo test -p sovemu-server schema_file`) and the clients.
- **Hot queries use an index.** A query that runs per request or per page view (sign-in, lists
  of games, Home's rows, a game's panel) ships with its index in the same migration, and joins
  `hot_queries` in `SovEmu.Server/src/query_plans.rs`, which fails on a plan that reads a whole
  table. Work over the whole library (labels, counts) is done when the library changes (after a
  scan or a lookup) and stored, not redone per request.
- **unsafe.** Only `LibretroFfi`, `RetroHost`, `Media` and `TestCore` contain `unsafe`. Every `unsafe`
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
  `cargo test --workspace`, all from `SovEmu.Backend/`. The pre-push hook (`.githooks/pre-push`,
  on with `git config core.hooksPath .githooks`) runs them for any push that touches the backend;
  never skip it with `--no-verify` to get a failing change up.
