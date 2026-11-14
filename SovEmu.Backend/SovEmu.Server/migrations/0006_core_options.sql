-- The options each core declared in its last game session, by core file name
-- (fceumm_libretro.so). Some cores declare their options only once a game is loaded, which
-- the probe (no game) cannot see; the Settings page falls back to these.
CREATE TABLE core_options (
    core       TEXT PRIMARY KEY,
    options    TEXT NOT NULL,  -- JSON: [CoreOption]
    categories TEXT NOT NULL,  -- JSON: [CoreCategory]
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
