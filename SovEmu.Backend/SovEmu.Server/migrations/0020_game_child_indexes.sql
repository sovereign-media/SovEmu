-- Each table that hangs off a game, by the game: removing a game (a rescan without its file)
-- cascades into all of them, and without an index each cascade reads the whole table, once per
-- game removed. Their primary keys lead with the user (or the collection), so none served it.
CREATE INDEX favourites_game ON favourites(game_id);
CREATE INDEX play_history_game ON play_history(game_id);
CREATE INDEX patch_choices_game ON patch_choices(game_id);
CREATE INDEX cheats_game ON cheats(game_id);
CREATE INDEX collection_games_game ON collection_games(game_id);
-- Removing a patch cascades into the choices of it, and the admin Patches page counts them.
CREATE INDEX patch_choices_patch ON patch_choices(patch_id);
