-- Home's "Recently added" row reads the newest games in this order: with the index it reads the
-- twenty it shows, not every game sorted.
CREATE INDEX games_added ON games(added_at DESC, id DESC);

-- What tells a game apart from same-titled games on its system ("Translation", "Rev 1"; see
-- releases.rs). Worked out after each scan and game-information lookup, so listing games only
-- reads it.
ALTER TABLE games ADD COLUMN release TEXT;
