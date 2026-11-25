-- Game information comes from FinalBurn Neo and ScreenScraper only: the LaunchBox Games
-- Database and TheGamesDB are no longer sources. What every source found is cleared, so each
-- game is looked up again from ScreenScraper, which matches files by hash. The box art files
-- the old rows downloaded are deleted by the server at startup (files, not rows), never a
-- game's own artwork in the library.
DELETE FROM game_metadata;

-- Which source filled each field, as JSON: {"title": "fbneo", "description": "screenscraper"}.
-- A refresh of some fields replaces only those; a source that finds nothing clears none.
ALTER TABLE game_metadata ADD COLUMN field_sources TEXT NOT NULL DEFAULT '{}';

-- Each source's own id for the game, as JSON: {"screenscraper": "1234"}, so a later lookup
-- asks for that game instead of hashing and matching the file again.
ALTER TABLE game_metadata ADD COLUMN provider_ids TEXT NOT NULL DEFAULT '{}';

-- A gameplay video, as box art is: where it is (video_url, never with credentials, and its
-- source), the downloaded file under <data>/metadata/media (video), and failed downloads.
ALTER TABLE game_metadata ADD COLUMN video_url TEXT;
ALTER TABLE game_metadata ADD COLUMN video_source TEXT;
ALTER TABLE game_metadata ADD COLUMN video TEXT;
ALTER TABLE game_metadata ADD COLUMN video_attempts INTEGER NOT NULL DEFAULT 0;
