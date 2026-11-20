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

-- A player's choice for a game: a patch, or the original (patch_id NULL). No row: the default.
CREATE TABLE patch_choices (
    user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    game_id  INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    patch_id INTEGER REFERENCES game_patches(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, game_id)
);
