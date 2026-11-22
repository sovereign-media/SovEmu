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
