-- What the admin's Games needing attention page reads beside the games themselves
-- (`attention.rs`), both kept by each library scan (`library.rs`).

-- A multi-part game's part numbers that are not in the library, comma-separated (`2`, `1,3`):
-- a gap in its numbering, or the parts before a lone later one. Empty for a whole set.
ALTER TABLE games ADD COLUMN missing_parts TEXT NOT NULL DEFAULT '';

-- The files the last scan could not place as a game, and why: replaced on every scan.
CREATE TABLE scan_skipped (
    -- Relative to the library, with `/` separators.
    path   TEXT PRIMARY KEY,
    -- The folder's system, if the folder is one's.
    system TEXT,
    reason TEXT NOT NULL
);
