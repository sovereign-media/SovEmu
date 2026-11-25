-- A game's artwork comes from ScreenScraper only: the artwork folders beside the ROMs
-- (Batocera's images/ and videos/) are no longer read, so the list of what the scan found
-- there goes.
ALTER TABLE games DROP COLUMN media;

-- A screenshot (shown behind the game: the "image" artwork) and the game's logo (the
-- "marquee"), as box art and videos are: where each is (never with credentials) and its
-- source, the downloaded file under <data>/metadata/media, and failed downloads.
ALTER TABLE game_metadata ADD COLUMN screenshot_url TEXT;
ALTER TABLE game_metadata ADD COLUMN screenshot_source TEXT;
ALTER TABLE game_metadata ADD COLUMN screenshot TEXT;
ALTER TABLE game_metadata ADD COLUMN screenshot_attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE game_metadata ADD COLUMN marquee_url TEXT;
ALTER TABLE game_metadata ADD COLUMN marquee_source TEXT;
ALTER TABLE game_metadata ADD COLUMN marquee TEXT;
ALTER TABLE game_metadata ADD COLUMN marquee_attempts INTEGER NOT NULL DEFAULT 0;

-- Games ScreenScraper found before it was asked for these are asked again (by the id it
-- gave, so not hashed again), keeping what they have: status 'refresh' reads as found until
-- the next lookup stores 'found' again.
UPDATE game_metadata SET status = 'refresh'
WHERE status = 'found' AND json_extract(provider_ids, '$.screenscraper') IS NOT NULL;
