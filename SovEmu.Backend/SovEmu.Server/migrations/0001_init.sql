-- Accounts. The first user, created through first-run setup, is the admin.
CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT    NOT NULL,
    is_admin      INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now'))
);

-- Signed-in browsers. Only a SHA-256 of the cookie's token is stored.
CREATE TABLE auth_sessions (
    token_hash TEXT    PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT    NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT    NOT NULL
);
CREATE INDEX auth_sessions_user ON auth_sessions(user_id);

-- The ROM library, as last scanned. `path` is relative to the ROM directory.
CREATE TABLE games (
    id       INTEGER PRIMARY KEY,
    system   TEXT    NOT NULL,
    path     TEXT    NOT NULL UNIQUE,
    title    TEXT    NOT NULL,
    tags     TEXT    NOT NULL DEFAULT '',
    size     INTEGER NOT NULL,
    modified INTEGER NOT NULL,
    added_at TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX games_system_title ON games(system, title COLLATE NOCASE);

-- What each user has played, for "last played" and ordering.
CREATE TABLE play_history (
    user_id        INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    game_id        INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    last_played_at TEXT    NOT NULL,
    play_count     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, game_id)
);
