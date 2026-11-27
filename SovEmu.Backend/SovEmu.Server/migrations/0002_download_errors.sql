-- Why a game's last download of each kind failed, shown in Needs attention beside the downloads
-- that gave up. Cleared when one succeeds, and when a lookup finds a different URL.
ALTER TABLE game_metadata ADD COLUMN box_art_error    TEXT;
ALTER TABLE game_metadata ADD COLUMN screenshot_error TEXT;
ALTER TABLE game_metadata ADD COLUMN marquee_error    TEXT;
ALTER TABLE game_metadata ADD COLUMN video_error      TEXT;
