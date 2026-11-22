-- Collections an admin keeps for everyone (no user), beside each user's own, and whether one was
-- declared in the environment (SOVEMU_COLLECTIONS__…: the admin page shows it locked). SQLite
-- cannot relax NOT NULL in place, so both tables are rebuilt with their rows. Dropping
-- `collections` would delete its games (ON DELETE CASCADE), so they are kept aside first.
CREATE TABLE collections_new (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    declared   INTEGER NOT NULL DEFAULT 0,
    created_at TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (user_id, name)
);
INSERT INTO collections_new (id, user_id, name, created_at)
    SELECT id, user_id, name, created_at FROM collections;

CREATE TABLE collection_games_kept AS SELECT collection_id, game_id, added_at FROM collection_games;
DROP TABLE collection_games;
DROP TABLE collections;
ALTER TABLE collections_new RENAME TO collections;

CREATE TABLE collection_games (
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    game_id       INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    added_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (collection_id, game_id)
);
INSERT INTO collection_games (collection_id, game_id, added_at)
    SELECT collection_id, game_id, added_at FROM collection_games_kept;
DROP TABLE collection_games_kept;
CREATE INDEX collection_games_game ON collection_games(game_id);

-- UNIQUE (user_id, name) treats every NULL user as different: the shared ones' names are unique
-- among themselves here.
CREATE UNIQUE INDEX collections_shared_name ON collections(name) WHERE user_id IS NULL;
