-- What each app sign-in calls its device ("SovEmu for Android 1.4.0 · Mi TV Stick"), as the app
-- names it once signed in (`nameDevice`), by refresh family: the admin's Devices page and a
-- user's details show it. A family that has ended loses its name at the next upkeep.
CREATE TABLE sign_in_devices (
    family_id TEXT PRIMARY KEY,
    name      TEXT NOT NULL,
    named_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
