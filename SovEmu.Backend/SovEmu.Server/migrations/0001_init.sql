-- SovEmu's database, as a new install starts it. A later change is a new numbered migration
-- beside this one; a migration that has shipped is never edited.

---- Accounts and sign-in ------------------------------------------------------------------------

-- Accounts. The first user, created through first-run setup, is an admin. A `viewer` plays, an
-- `admin` also runs the server.
CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT    NOT NULL,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    role          TEXT    NOT NULL DEFAULT 'viewer' CHECK (role IN ('viewer', 'admin'))
);

-- Browsers signed in at the server's OpenID Connect provider (the cookie `/oauth/authorize`
-- reads). Only a SHA-256 of the cookie's token is stored.
CREATE TABLE auth_sessions (
    token_hash TEXT    PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT    NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT    NOT NULL
);
CREATE INDEX auth_sessions_user ON auth_sessions(user_id);

-- The authorization server's storage (oauth-as's `Storage`, `SovEmu.Identity/src/store.rs`).
-- Each record is kept whole as JSON in `record`, beside the columns it is looked up, cascaded
-- and swept by. Instants are Unix nanoseconds. Access tokens, refresh tokens and authorization
-- codes are keyed by the SHA-256 (hex) of the credential, and the credential is left out of
-- `record`. `subject` is the user id as text: what ending every sign-in of a user deletes by.
-- There are no foreign keys: oauth-as decides what a subject or a client is, and its
-- revocations do the cascading.

-- Registered clients (applications that may sign in).
CREATE TABLE oauth_clients (
    client_id TEXT PRIMARY KEY,
    record    TEXT NOT NULL
);

-- RFC 8628 device grants. `user_code` is the normalized code a person types, unique so that one
-- code never answers for two devices; `subject` is set once the grant is approved.
CREATE TABLE oauth_device_grants (
    device_code TEXT    PRIMARY KEY,
    user_code   TEXT    NOT NULL UNIQUE,
    client_id   TEXT    NOT NULL,
    subject     TEXT,
    state       TEXT    NOT NULL,
    expires_at  INTEGER NOT NULL,
    record      TEXT    NOT NULL
);
CREATE INDEX oauth_device_grants_client ON oauth_device_grants(client_id);
CREATE INDEX oauth_device_grants_subject ON oauth_device_grants(subject);
CREATE INDEX oauth_device_grants_expiry ON oauth_device_grants(expires_at);

-- Authorization codes, in either state (a consumed code is kept until it expires so a replay is
-- recognised). `state_digest` is the SHA-256 of the state, what a compare-and-swap compares.
CREATE TABLE oauth_authorization_codes (
    code_hash    TEXT    PRIMARY KEY,
    client_id    TEXT    NOT NULL,
    subject      TEXT    NOT NULL,
    state_digest TEXT    NOT NULL,
    expires_at   INTEGER NOT NULL,
    record       TEXT    NOT NULL
);
CREATE INDEX oauth_authorization_codes_client ON oauth_authorization_codes(client_id);
CREATE INDEX oauth_authorization_codes_subject ON oauth_authorization_codes(subject);
CREATE INDEX oauth_authorization_codes_expiry ON oauth_authorization_codes(expires_at);

-- Access tokens. `family_id` is the refresh chain the token was minted along, if any.
CREATE TABLE oauth_access_tokens (
    token_hash TEXT    PRIMARY KEY,
    client_id  TEXT    NOT NULL,
    subject    TEXT,
    family_id  TEXT,
    expires_at INTEGER NOT NULL,
    record     TEXT    NOT NULL
);
CREATE INDEX oauth_access_tokens_client ON oauth_access_tokens(client_id);
CREATE INDEX oauth_access_tokens_subject ON oauth_access_tokens(subject);
CREATE INDEX oauth_access_tokens_family ON oauth_access_tokens(family_id);
CREATE INDEX oauth_access_tokens_expiry ON oauth_access_tokens(expires_at);

-- Refresh tokens, live and spent (a spent one is kept until `expires_at` so reuse is detected;
-- a chain with no absolute lifetime has none).
CREATE TABLE oauth_refresh_tokens (
    token_hash TEXT    PRIMARY KEY,
    client_id  TEXT    NOT NULL,
    subject    TEXT,
    family_id  TEXT    NOT NULL,
    expires_at INTEGER,
    record     TEXT    NOT NULL
);
CREATE INDEX oauth_refresh_tokens_client ON oauth_refresh_tokens(client_id);
CREATE INDEX oauth_refresh_tokens_subject ON oauth_refresh_tokens(subject);
CREATE INDEX oauth_refresh_tokens_family ON oauth_refresh_tokens(family_id);
CREATE INDEX oauth_refresh_tokens_expiry ON oauth_refresh_tokens(expires_at);

-- What a revocation removed, so that an issuance already in flight cannot write it back: a
-- deleted client, a revoked refresh family, a withdrawn consent (`scope` the client, `subject`
-- the user) or every sign-in of one user (`scope` the user id). Swept at `until`.
CREATE TABLE oauth_revocation_barriers (
    kind        TEXT    NOT NULL CHECK (kind IN ('client', 'family', 'consent', 'subject')),
    scope       TEXT    NOT NULL CHECK (scope <> ''),
    subject     TEXT    NOT NULL DEFAULT '',
    recorded_at INTEGER NOT NULL,
    until       INTEGER NOT NULL,
    PRIMARY KEY (kind, scope, subject)
);
CREATE INDEX oauth_revocation_barriers_until ON oauth_revocation_barriers(until);

-- What each app sign-in calls its device ("SovEmu for Android 1.4.0 · Mi TV Stick"), as the app
-- names it once signed in (`nameDevice`), by refresh family: the admin's Devices page and a
-- user's details show it. A family that has ended loses its name at the next upkeep.
CREATE TABLE sign_in_devices (
    family_id TEXT PRIMARY KEY,
    name      TEXT NOT NULL,
    named_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- What each sign-in's device says it can decode and show (`sessionOffer`'s `capabilities`), by
-- refresh family: one complete, versioned JSON document, replaced whole each time the device
-- starts a stream. The server picks a session's video codec from it, and the admin's lists of
-- sign-ins show it. A family that has ended loses its document at the next upkeep.
CREATE TABLE sign_in_capabilities (
    family_id   TEXT PRIMARY KEY,
    document    TEXT NOT NULL,
    reported_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- What each sign-in's device measured its video decoder to take a frame, by codec: the mean of
-- its latest session with that codec, as its once-a-second stats reported it. The server
-- prices a codec for a device by this where it has it, over what the device declares
-- (`sign_in_capabilities`): a declared hardware decoder can still be the slow one. Rows of
-- sign-ins that have ended go when a new measurement is kept.
CREATE TABLE device_decode_times (
    family_id   TEXT NOT NULL,
    codec       TEXT NOT NULL,
    decode_ms   REAL NOT NULL,
    samples     INTEGER NOT NULL,
    measured_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (family_id, codec)
);

---- The library ---------------------------------------------------------------------------------

-- The ROM library, as last scanned. `path` is relative to the ROM directory.
CREATE TABLE games (
    id            INTEGER PRIMARY KEY,
    system        TEXT    NOT NULL,
    path          TEXT    NOT NULL UNIQUE,
    title         TEXT    NOT NULL,
    tags          TEXT    NOT NULL DEFAULT '',
    size          INTEGER NOT NULL,
    modified      INTEGER NOT NULL,
    added_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    -- What tells a game apart from same-titled games on its system ("Translation", "Rev 1"; see
    -- releases.rs). Worked out after each scan and game-information lookup, so listing games
    -- only reads it.
    release       TEXT,
    -- What a multi-part game's files are (`disc`, `disk`, `tape`, `side` or `part`; see
    -- library.rs): the clients show its count with the right icon. NULL for a single file.
    parts         TEXT,
    -- A multi-part game's part numbers that are not in the library, comma-separated (`2`,
    -- `1,3`): a gap in its numbering, or the parts before a lone later one. Empty for a whole
    -- set. The admin's Games needing attention page reads it (`attention.rs`).
    missing_parts TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX games_system_title ON games(system, title COLLATE NOCASE);
-- Home's "Recently added" row reads the newest games in this order: with the index it reads the
-- twenty it shows, not every game sorted.
CREATE INDEX games_added ON games(added_at DESC, id DESC);

-- A multi-disc game's discs, in order: files that differ only by a "(Disc N)" tag are one
-- game (its row is disc 1's path), and this lists every disc's file.
CREATE TABLE game_discs (
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    disc    INTEGER NOT NULL,
    path    TEXT    NOT NULL,
    PRIMARY KEY (game_id, disc)
);

-- Soft patches (IPS, UPS, BPS) found with games by the library scan: applied as a game loads,
-- never written to the library. Kept by path across scans, so a player's choice stays put.
CREATE TABLE game_patches (
    id         INTEGER PRIMARY KEY,
    game_id    INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    -- Relative to the ROM directory.
    path       TEXT    NOT NULL UNIQUE,
    name       TEXT    NOT NULL,
    -- Named as its game (`Game.bps` beside `Game.sfc`): applied unless the player chose
    -- otherwise, as RetroArch does. At most one per game.
    is_default INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX game_patches_game ON game_patches(game_id);

-- The files the last scan could not place as a game, and why: replaced on every scan. The
-- admin's Games needing attention page reads it (`attention.rs`).
CREATE TABLE scan_skipped (
    -- Relative to the library, with `/` separators.
    path   TEXT PRIMARY KEY,
    -- The folder's system, if the folder is one's.
    system TEXT,
    reason TEXT NOT NULL
);

-- Game information from FinalBurn Neo's DATs and ScreenScraper, merged. One row per game once
-- it has been looked up, found or not, so a job does not ask again.
--
-- Artwork (box art, a screenshot, the game's logo or "marquee") and a gameplay video are
-- downloaded by a background job of their own, after the lookup that found them, so a lookup
-- stores a game's information at once. For each: where it is (`*_url`, never with credentials:
-- ScreenScraper's are stripped before it is stored and added again when downloading, which is
-- why `*_source` is kept beside it), the downloaded file under <data>/artwork/games
-- (`<game id>/<name>`), and failed downloads (`*_attempts`: the job gives up after a few, and a
-- new URL starts again at 0).
CREATE TABLE game_metadata (
    game_id             INTEGER PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE,
    -- "found", "not_found", or "refresh": a lookup that a stopped source (ScreenScraper out of
    -- quota) left unfinished, kept to be asked again (by the id the source gave, so not hashed
    -- again) and reading as found until then.
    status              TEXT    NOT NULL,
    -- The services that contributed, in order, e.g. "fbneo,screenscraper".
    sources             TEXT    NOT NULL DEFAULT '',
    title               TEXT,
    description         TEXT,
    release_date        TEXT,
    developer           TEXT,
    publisher           TEXT,
    genre               TEXT,
    players             TEXT,
    -- 0 to 1.
    rating              REAL,
    box_art             TEXT,
    fetched_at          TEXT    NOT NULL DEFAULT (datetime('now')),
    -- The services asked about a game, e.g. "fbneo,screenscraper". A game no service found is
    -- asked again once a service it was not asked is configured, so adding credentials later
    -- fills it in without anyone having to fetch everything again.
    tried               TEXT    NOT NULL DEFAULT '',
    box_art_url         TEXT,
    box_art_source      TEXT,
    box_art_attempts    INTEGER NOT NULL DEFAULT 0,
    -- Which source filled each field, as JSON: {"title": "fbneo", "description":
    -- "screenscraper"}. A refresh of some fields replaces only those; a source that finds
    -- nothing clears none.
    field_sources       TEXT    NOT NULL DEFAULT '{}',
    -- Each source's own id for the game, as JSON: {"screenscraper": "1234"}, so a later lookup
    -- asks for that game instead of hashing and matching the file again.
    provider_ids        TEXT    NOT NULL DEFAULT '{}',
    video_url           TEXT,
    video_source        TEXT,
    video               TEXT,
    video_attempts      INTEGER NOT NULL DEFAULT 0,
    -- Shown behind the game (the "image" artwork).
    screenshot_url      TEXT,
    screenshot_source   TEXT,
    screenshot          TEXT,
    screenshot_attempts INTEGER NOT NULL DEFAULT 0,
    marquee_url         TEXT,
    marquee_source      TEXT,
    marquee             TEXT,
    marquee_attempts    INTEGER NOT NULL DEFAULT 0,
    -- The ESRB age rating ("EC", "E", "E10+", "T", "M", "AO" or "RP"), and the play modes,
    -- series and themes (each a comma-separated list of names, as genre is).
    esrb                TEXT,
    modes               TEXT,
    series              TEXT,
    themes              TEXT
);
-- Home's top rated row reads the best rated first, and stops at its length.
CREATE INDEX game_metadata_rating ON game_metadata (rating DESC) WHERE rating IS NOT NULL;

---- Players' own -------------------------------------------------------------------------------
-- Each table that hangs off a game has an index by the game: removing a game (a rescan without
-- its file) cascades into all of them, and without one each cascade reads the whole table, once
-- per game removed. Their primary keys lead with the user (or the collection), so none serves it.

-- What each user has played, for "last played" and ordering.
CREATE TABLE play_history (
    user_id        INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    game_id        INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    last_played_at TEXT    NOT NULL,
    play_count     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, game_id)
);
CREATE INDEX play_history_game ON play_history(game_id);

-- Cheat codes a user keeps for a game. The worker applies the enabled ones in order.
CREATE TABLE cheats (
    id          INTEGER PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    game_id     INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    description TEXT    NOT NULL DEFAULT '',
    code        TEXT    NOT NULL,
    enabled     INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX cheats_user_game ON cheats(user_id, game_id);
CREATE INDEX cheats_game ON cheats(game_id);

-- A player's choice of patch for a game: a patch, or the original (patch_id NULL). No row: the
-- default.
CREATE TABLE patch_choices (
    user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    game_id  INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    patch_id INTEGER REFERENCES game_patches(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, game_id)
);
CREATE INDEX patch_choices_game ON patch_choices(game_id);
-- Removing a patch cascades into the choices of it, and the admin Patches page counts them.
CREATE INDEX patch_choices_patch ON patch_choices(patch_id);

-- A user's favourite games.
CREATE TABLE favourites (
    user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    game_id  INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    added_at TEXT    NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, game_id)
);
CREATE INDEX favourites_game ON favourites(game_id);

-- Named collections of games: a user's own, or (no user) one an admin keeps for everyone.
-- `declared`: it comes from the environment (SOVEMU_COLLECTIONS__…), and the admin page shows
-- it locked.
CREATE TABLE collections (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    declared   INTEGER NOT NULL DEFAULT 0,
    created_at TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (user_id, name)
);
-- UNIQUE (user_id, name) treats every NULL user as different: the shared ones' names are unique
-- among themselves here.
CREATE UNIQUE INDEX collections_shared_name ON collections(name) WHERE user_id IS NULL;

CREATE TABLE collection_games (
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    game_id       INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    added_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (collection_id, game_id)
);
CREATE INDEX collection_games_game ON collection_games(game_id);

-- A user's button mapping for one controller model (or the keyboard), shared by every client,
-- for one system or ('') every system. `controller` is an ID from docs/CONTROLLERS.md;
-- `bindings` is JSON: [{"input": "south", "button": "B"}, …]. A row replaces the defaults for
-- that controller.
CREATE TABLE controller_mappings (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    controller TEXT    NOT NULL,
    system     TEXT    NOT NULL DEFAULT '',
    name       TEXT    NOT NULL DEFAULT '',
    bindings   TEXT    NOT NULL,
    updated_at TEXT    NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, controller, system)
);

---- What admins choose -------------------------------------------------------------------------

-- An admin's default mapping for a controller model, for every user without their own, for one
-- system or ('') every system. The environment (SOVEMU_CONTROLLERS__<n>__…) wins over a row
-- here.
CREATE TABLE controller_defaults (
    controller TEXT NOT NULL,
    system     TEXT NOT NULL DEFAULT '',
    name       TEXT NOT NULL DEFAULT '',
    bindings   TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (controller, system)
);

-- Emulator settings an admin chose in the web app, per system: libretro core options
-- (`key` = option name) applied when a game of that system starts. SovEmu's own defaults sit
-- beneath them and SOVEMU_SYSTEMS__<id>__OPTIONS__<name> above them: the environment always
-- wins, so these are a shortcut, never the only way to set an option.
CREATE TABLE system_settings (
    system     TEXT NOT NULL,
    key        TEXT NOT NULL,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (system, key)
);

-- Run-ahead an admin chose in the web app, per system: how many frames its games' pictures run
-- ahead of the game, hiding the game's own input lag (0: off). SOVEMU_SYSTEMS__<id>__RUN_AHEAD
-- sits above it and always wins; without either it is off.
CREATE TABLE system_run_ahead (
    system     TEXT PRIMARY KEY,
    frames     INTEGER NOT NULL CHECK (frames >= 0),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Settings the admin chooses for the whole server that are not a system's (`system_settings`):
-- the background behind every screen (`appearance.rs`). The environment's value, where set, wins
-- and is never stored here.
CREATE TABLE server_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- The options each core declared in its last game session, by core file name
-- (fceumm_libretro.so). Some cores declare their options only once a game is loaded, which
-- the probe (no game) cannot see; the Settings page falls back to these.
CREATE TABLE core_options (
    core       TEXT PRIMARY KEY,
    options    TEXT NOT NULL,  -- JSON: [CoreOption]
    categories TEXT NOT NULL,  -- JSON: [CoreCategory]
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

---- What the server records ---------------------------------------------------------------------

-- Every run of a background job (`jobs.rs`): a library scan, a mirror sync, a game-information
-- lookup, database upkeep. A row is written as a run starts (`running`) and finished when it
-- ends; one still `running` at startup was cut short by the server stopping (`interrupted`).
-- Maintenance deletes rows older than SOVEMU_JOB_HISTORY_DAYS.
CREATE TABLE job_runs (
    id          INTEGER PRIMARY KEY,
    job         TEXT NOT NULL,
    -- `startup`, `schedule` or `admin`.
    trigger     TEXT NOT NULL,
    -- `running`, `succeeded`, `failed`, `cancelled` or `interrupted`.
    outcome     TEXT NOT NULL DEFAULT 'running',
    started_at  TEXT NOT NULL DEFAULT (datetime('now')),
    finished_at TEXT,
    duration_ms INTEGER,
    -- What the run did ("1,204 games: 3 added, 0 removed") or why it failed.
    message     TEXT
);
-- The admin Jobs page: each job's latest runs.
CREATE INDEX job_runs_by_job ON job_runs (job, id);
-- Maintenance's pruning.
CREATE INDEX job_runs_by_start ON job_runs (started_at);

-- The container's CPU and memory use, one row per resource per minute (`resources/mod.rs`): the
-- minute's average and highest figure, as a share (0–100) of the allowed cores or the memory
-- limit. The admin Resources page charts them; maintenance deletes rows older than
-- SOVEMU_RESOURCE_HISTORY_DAYS.
CREATE TABLE resource_samples (
    id       INTEGER PRIMARY KEY,
    -- `cpu` or `memory`.
    resource TEXT NOT NULL,
    -- Unix seconds, the end of the minute: charts bucket by it.
    at       INTEGER NOT NULL,
    average  REAL NOT NULL,
    max      REAL NOT NULL
);
-- Charts read a window by time; maintenance prunes by it.
CREATE INDEX resource_samples_by_time ON resource_samples (at);

-- What went wrong on the clients (`client_events.rs`): what each app reported with
-- `reportClientEvents` and the server accepted (within the user's allowance, with
-- SOVEMU_CLIENT_EVENTS on). The admin Client errors page groups them; maintenance deletes rows
-- older than SOVEMU_CLIENT_EVENT_DAYS.
CREATE TABLE client_events (
    id         INTEGER PRIMARY KEY,
    -- Unix seconds, when the server received it.
    at         INTEGER NOT NULL,
    -- Unix seconds on the client's clock, if it said.
    client_at  REAL,
    -- The account it came from: kept when the account is deleted, which the page shows.
    user_id    INTEGER NOT NULL,
    -- The app and its version, as it named itself (`android 1.4.0 (Mi TV, Android 11)`).
    client     TEXT NOT NULL,
    -- `connection_failed`, `connection_lost`, `stream_stalled`, `controller_unknown`,
    -- `screen_error` or `other`.
    kind       TEXT NOT NULL,
    message    TEXT NOT NULL,
    -- The game session it happened in, and that session's game (if it was still running when
    -- the event arrived).
    session_id TEXT,
    game_id    INTEGER
);
-- The page reads a window by time (grouped, or one group's events); maintenance prunes by it.
CREATE INDEX client_events_by_time ON client_events (at);

-- The activity log (`activity.rs`): who did what, and when. Sign-ins and refusals, games
-- started and ended, and what admins changed. The admin Activity page reads it newest first,
-- by user and by kind; maintenance deletes rows older than SOVEMU_ACTIVITY_HISTORY_DAYS.
CREATE TABLE activity (
    id       INTEGER PRIMARY KEY,
    -- Unix seconds.
    at       INTEGER NOT NULL,
    -- The account that did it: kept when the account is deleted (the name stays beside it).
    -- Null for a refused sign-in.
    user_id  INTEGER,
    -- The name as given: for a refused sign-in, the name that was tried.
    username TEXT,
    -- `signed_in`, `sign_in_failed`, `game_started`, `role_changed`, … (`activity::Kind`), and
    -- the page's filter it falls under: `sign_in`, `play` or `admin`.
    kind     TEXT NOT NULL,
    category TEXT NOT NULL,
    -- What it was about (a game, an account, a job) and more about it (a role, an end reason).
    -- Never a password, a token or a secret.
    subject  TEXT,
    detail   TEXT,
    -- The client's address, where there was a request.
    address  TEXT
);
-- The page: newest first, all of it, by kind, or by user; maintenance prunes by time.
CREATE INDEX activity_by_time ON activity (at, id);
CREATE INDEX activity_by_category ON activity (category, at, id);
CREATE INDEX activity_by_user ON activity (user_id, at, id);

-- Every game played (`play_stats.rs`): one row per session whose game loaded, written as it
-- ends, for the admin Statistics page (most played games, hours played, by account, by system,
-- and over time). `play_history` is one row per account and game, for Home.
CREATE TABLE play_sessions (
    id         INTEGER PRIMARY KEY,
    -- The account: kept when it is deleted, so totals do not change (the page names it then as
    -- a deleted account).
    user_id    INTEGER NOT NULL,
    -- The game, while it is in the library; its title and system are kept for when it is not.
    game_id    INTEGER REFERENCES games(id) ON DELETE SET NULL,
    title      TEXT    NOT NULL,
    system     TEXT    NOT NULL,
    -- The core's file name without its extension.
    core       TEXT    NOT NULL,
    -- Unix seconds.
    started_at INTEGER NOT NULL,
    ended_at   INTEGER NOT NULL,
    seconds    INTEGER NOT NULL,
    -- How it ended: `stopped`, `idle`, `crashed`, … (`sovemu_sessions::EndReason`).
    end_reason TEXT    NOT NULL
);
-- The page reads a window by start time; removing a game sets its rows' game_id to NULL.
CREATE INDEX play_sessions_by_start ON play_sessions (started_at);
CREATE INDEX play_sessions_game ON play_sessions (game_id);
