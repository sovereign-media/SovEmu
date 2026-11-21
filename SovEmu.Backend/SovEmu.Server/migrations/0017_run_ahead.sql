-- Run-ahead an admin chose in the web app, per system: how many frames its games' pictures run
-- ahead of the game, hiding the game's own input lag (0: off). SOVEMU_SYSTEMS__<id>__RUN_AHEAD
-- sits above it and always wins; without either it is off.
CREATE TABLE system_run_ahead (
    system     TEXT PRIMARY KEY,
    frames     INTEGER NOT NULL CHECK (frames >= 0),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
