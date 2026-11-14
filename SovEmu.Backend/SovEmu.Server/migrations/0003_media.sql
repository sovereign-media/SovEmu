-- Artwork and video found beside a ROM (Batocera/EmulationStation layout:
-- images/<rom>-image.png, -thumb.png, -marquee.png and videos/<rom>-video.mp4), as a
-- comma-separated list of kinds: "image,thumb,marquee,video".
ALTER TABLE games ADD COLUMN media TEXT NOT NULL DEFAULT '';
