-- Game information from metadata services (FBNeo's DATs, ScreenScraper, TheGamesDB), merged.
-- One row per game once it has been looked up, found or not, so a job does not ask again.
CREATE TABLE game_metadata (
    game_id      INTEGER PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE,
    -- "found" or "not_found".
    status       TEXT    NOT NULL,
    -- The services that contributed, in order, e.g. "fbneo,screenscraper".
    sources      TEXT    NOT NULL DEFAULT '',
    title        TEXT,
    description  TEXT,
    release_date TEXT,
    developer    TEXT,
    publisher    TEXT,
    genre        TEXT,
    players      TEXT,
    -- 0 to 1.
    rating       REAL,
    -- Downloaded box art, a file name under <data dir>/metadata/media.
    box_art      TEXT,
    fetched_at   TEXT    NOT NULL DEFAULT (datetime('now'))
);
