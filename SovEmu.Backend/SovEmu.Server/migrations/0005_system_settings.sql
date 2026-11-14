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
