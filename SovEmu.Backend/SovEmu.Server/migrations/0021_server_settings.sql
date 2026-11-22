-- Settings the admin chooses for the whole server that are not a system's (`system_settings`):
-- the background behind every screen (`appearance.rs`). The environment's value, where set, wins
-- and is never stored here.
CREATE TABLE server_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
