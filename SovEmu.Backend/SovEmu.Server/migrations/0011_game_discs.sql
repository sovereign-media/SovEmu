-- A multi-disc game's discs, in order: files that differ only by a "(Disc N)" tag are one
-- game (its row is disc 1's path), and this lists every disc's file.
CREATE TABLE game_discs (
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    disc    INTEGER NOT NULL,
    path    TEXT    NOT NULL,
    PRIMARY KEY (game_id, disc)
);
