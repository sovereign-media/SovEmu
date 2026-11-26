---
paths:
  - "SovEmu.Backend/SovEmu.Server/**"
  - "SovEmu.Backend/SovEmu.Sessions/**"
  - "SovEmu.Backend/SovEmu.Identity/**"
  - "SovEmu.Backend/SovEmu.Assets/**"
---

# Server, sessions, identity and assets

## The server never runs a core

Cores and codecs live only in `sovemu-worker`. `Sessions` starts one `sovemu-worker session` per
game with a UDP port from the pool and talks to it over JSON lines; the server never links
`RetroHost` or `Media`.

- **Workers get no secrets.** Every worker launch clears the environment and passes only
  `worker_env()` (`Sessions/src/workers.rs`, `WORKER_ENV` is the allow-list): `worker_command`,
  `gpu.rs`, `encoders.rs`, and the server's own `probe` in `settings.rs`. A new launch does the
  same; pass what a worker needs as arguments.
- **Never kill a worker without first asking it to shut down**: `Workers::stop` does both, in
  that order, so saves are written.
- **SIGTERM shuts down as Ctrl-C does**: stop every worker, then exit (Kubernetes and
  `docker stop` send SIGTERM).
- Each session's end (what it played, whether it loaded, why it ended, its last lines) goes to the
  `Observer`: the server's metrics and failures list.

## The API is GraphQL, and only GraphQL

- `POST /graphql` for every client, GraphiQL at `GET /graphql`, async-graphql in
  `SovEmu.Server/src/graphql/`: one module per area, each with its types and its part of the
  `Query` and `Mutation` roots, joined in `mod.rs`. **Never add a REST route.**
- Resolvers check access (`user(ctx)`, `admin(ctx)`) and call plain functions (`&AppState`,
  `&User`, arguments) in the other modules. Errors carry `extensions.code`.
- `Int` is 32-bit, so byte counts and Unix times are `Float`.
- After a schema change, `UPDATE_SCHEMA=1 cargo test -p sovemu-server schema_file` rewrites
  `schema.graphql`, and every client changes with it (see [contracts.md](contracts.md)).
- Plain HTTP is only for files the API hands out URLs to (`/media/…`), for sign-in
  (`/.well-known/openid-configuration`, `/oauth/…`, and `/device`, which only redirects to the
  web client's `/link/`), for `/health`, and for the web client's static files (every other
  page path answers `index.html`; no Node, no SSR).

## Signed file URLs

Every `/media/…` URL is **signed** for the requesting user (`state.assets.sign`,
`SovEmu.Assets`: HMAC-SHA256, `Sov-UserId`/`Sov-Expiry`/`Sov-Signature`, Sovereign's scheme). A
`/media/` route sits behind `require_signature` and serves the signed user's data, never a
cookie's. Never hand out or serve an unsigned file URL.

## Sign-in (`SovEmu.Identity`)

- Built on `oauth-as` (OAuth 2.1) with OpenID Connect over it. **Never hand-roll protocol logic
  oauth-as owns** (codes, PKCE, refresh rotation and reuse detection, revocation, the device
  grant): extend it around the edges. `SqliteStorage` must keep passing oauth-as's conformance
  harness.
- The API is signed in only by `Authorization: Bearer` access tokens, never a cookie. The
  `sovemu_session` cookie is the browser's sign-in at the server, read only in front of
  `/oauth/authorize`. A TV's code is approved by an access token, through `approveDeviceCode`:
  from the phone apps (*Sign in a TV*), or the web client's `/link/` page, which is the
  `verificationUri` a TV shows. oauth-as's own `/device` form is not used.
- **Three clients**, as Sovereign's, each id per install (`data/server.json`, never a setting):
  **web** (the code flow with PKCE, never a password), **device** (TVs and any app signing in
  with the device grant or the password fallback, `/oauth/password`,
  `SovEmu__Authentication__PasswordSignIn`) and **mobile** (phones through the system browser, to
  `media.sovereign.sovemu:/oauth2redirect`). Clients learn every id and sign-in URL from the
  anonymous `bootstrapConfig` query and hard-code none. A replaced id's registration is removed
  with its sign-ins on start.
- **Sign in a TV**: an app signed in already approves a TV's code with `approveDeviceCode`
  (GraphQL, where Sovereign has `POST /connect/verify`), through `Identity::approve_device`
  (oauth-as's `approve_device`; wrong codes throttled per user).
- Roles are `viewer` and `admin` (`users.role`), checked on every request.

## Database

- SQLite through sqlx (WAL). Queries are `sqlx::query` / `query_as` / `query_scalar` with bound
  parameters; never format a value into SQL (identifiers chosen by code, as in `Identity`'s store,
  are the only text formatted in).
- **Schema changes are a new numbered file in `SovEmu.Server/migrations/`**; never SQL in Rust,
  and never edit a migration that has shipped.
- **Hot queries use an index.** A query run per request or per page view (sign-in, game lists,
  Home's rows, a game's panel) ships with its index in the same migration and joins `hot_queries`
  in `query_plans.rs`, which fails on a plan that reads a whole table. Work over the whole library
  (labels, counts) is done when the library changes and stored, not redone per request.
- The data folder's layout is `paths.rs` (`config.paths()`): a new file or folder gets a method
  there, never a `data_dir.join("…")` elsewhere.

## Background work is a job

`SovEmu.Server/src/jobs.rs`: a `Job` with its interval from a `SovEmu__*` setting, run through
`Jobs`, which runs one of a job at a time, makes jobs in a lane (scan, mirror sync) wait for each
other, records every run in `job_runs`, turns a panic into a failed run and cancels everything on
shutdown. Never start a bare `tokio::spawn` loop for scheduled or admin-started work; an admin's
button calls `runJob` or the job's own mutation.

## Telemetry

Pushed, never served: traces and metrics go out over OTLP/HTTP (`telemetry.rs`) only when
`SovEmu__Telemetry__Otlp__Endpoint` is set, and there is no metrics route. New metrics are
instruments in `telemetry.rs`; new operational routes stay off unless a setting turns them on.
