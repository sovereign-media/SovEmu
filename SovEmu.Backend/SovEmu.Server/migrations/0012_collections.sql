-- A user's favourite games, and their named collections of games.
CREATE TABLE favourites (
    user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    game_id  INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    added_at TEXT    NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, game_id)
);

CREATE TABLE collections (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    created_at TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (user_id, name)
);

CREATE TABLE collection_games (
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    game_id       INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    added_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (collection_id, game_id)
);
