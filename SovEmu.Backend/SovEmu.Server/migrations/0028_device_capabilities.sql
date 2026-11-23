-- What each sign-in's device says it can decode and show (`sessionOffer`'s `capabilities`), by
-- refresh family: one complete, versioned JSON document, replaced whole each time the device
-- starts a stream. The server picks a session's video codec from it, and the admin's lists of
-- sign-ins show it. A family that has ended loses its document at the next upkeep.
CREATE TABLE sign_in_capabilities (
    family_id   TEXT PRIMARY KEY,
    document    TEXT NOT NULL,
    reported_at TEXT NOT NULL DEFAULT (datetime('now'))
);
