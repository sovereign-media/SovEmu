-- Box art is downloaded by its own background job, after the lookup that found it, so a lookup
-- stores a game's information at once. A lookup records where the box art is; the job
-- downloads it (into box_art) for the games that will show it (no local thumb).
--
-- box_art_url never holds credentials: ScreenScraper's are stripped before it is stored and
-- added again when downloading, which is why the source is kept beside it.
ALTER TABLE game_metadata ADD COLUMN box_art_url TEXT;
-- The source of box_art_url ("launchbox", "screenscraper", "thegamesdb").
ALTER TABLE game_metadata ADD COLUMN box_art_source TEXT;
-- Failed downloads of box_art_url; the job gives up after a few. A new URL starts again at 0.
ALTER TABLE game_metadata ADD COLUMN box_art_attempts INTEGER NOT NULL DEFAULT 0;
