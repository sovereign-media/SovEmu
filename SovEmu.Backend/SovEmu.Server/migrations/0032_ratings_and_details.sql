-- More of what ScreenScraper knows about a game: its ESRB age rating ("EC", "E", "E10+", "T",
-- "M", "AO" or "RP"), and its play modes, series and themes (each a comma-separated list of
-- names, as genre is).
ALTER TABLE game_metadata ADD COLUMN esrb TEXT;
ALTER TABLE game_metadata ADD COLUMN modes TEXT;
ALTER TABLE game_metadata ADD COLUMN series TEXT;
ALTER TABLE game_metadata ADD COLUMN themes TEXT;

-- Home's top rated row reads the best rated first, and stops at its length.
CREATE INDEX game_metadata_rating ON game_metadata (rating DESC) WHERE rating IS NOT NULL;

-- Games ScreenScraper found before it was asked for these are asked again (by its id, so not
-- hashed again), keeping what they have, as in migration 0031.
UPDATE game_metadata SET status = 'refresh'
WHERE status = 'found' AND json_extract(provider_ids, '$.screenscraper') IS NOT NULL;
