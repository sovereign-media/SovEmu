-- Cheat codes a user keeps for a game. The worker applies the enabled ones in order.
CREATE TABLE cheats (
    id          INTEGER PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    game_id     INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    description TEXT    NOT NULL DEFAULT '',
    code        TEXT    NOT NULL,
    enabled     INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX cheats_user_game ON cheats(user_id, game_id);
