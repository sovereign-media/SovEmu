-- Every game played (`play_stats.rs`): one row per session whose game loaded, written as it
-- ends, for the admin Statistics page (most played games, hours played, by account, by system,
-- and over time). `play_history` stays as it is: one row per account and game, for Home.
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
