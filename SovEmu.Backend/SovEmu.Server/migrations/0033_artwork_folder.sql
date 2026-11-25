-- Downloaded game art moved from <data>/metadata/media/<game id>-<name> into a folder per game,
-- <data>/artwork/games/<game id>/<name> (the server moves the files at startup): the file names
-- the table keeps become "<game id>/<name>" ("12-box.webp" → "12/box.webp").
UPDATE game_metadata SET box_art = replace(box_art, game_id || '-', game_id || '/')
WHERE box_art LIKE game_id || '-%';
UPDATE game_metadata SET screenshot = replace(screenshot, game_id || '-', game_id || '/')
WHERE screenshot LIKE game_id || '-%';
UPDATE game_metadata SET marquee = replace(marquee, game_id || '-', game_id || '/')
WHERE marquee LIKE game_id || '-%';
UPDATE game_metadata SET video = replace(video, game_id || '-', game_id || '/')
WHERE video LIKE game_id || '-%';
