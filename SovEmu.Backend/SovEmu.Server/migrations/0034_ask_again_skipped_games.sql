-- A lookup that skipped a stopped source (ScreenScraper out of quota) stored its games as looked
-- up, so they were never asked again: after 0032 asked every game for its ESRB rating, play modes,
-- series and themes, the quota ran out and the games after it (most of PlayStation and SNES) kept
-- none of them. Lookups now keep such games to ask again; these are asked again too (by the id
-- ScreenScraper gave, so not hashed again), keeping what they have.
UPDATE game_metadata SET status = 'refresh'
WHERE status = 'found'
  AND json_extract(provider_ids, '$.screenscraper') IS NOT NULL
  AND esrb IS NULL AND modes IS NULL AND series IS NULL AND themes IS NULL;
