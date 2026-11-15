-- A user's button mapping for one controller model (or the keyboard), shared by every client.
-- `controller` is an ID from docs/CONTROLLERS.md; `bindings` is JSON:
-- [{"input": "south", "button": "B"}, …]. A row replaces the defaults for that controller.
CREATE TABLE controller_mappings (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    controller TEXT    NOT NULL,
    name       TEXT    NOT NULL DEFAULT '',
    bindings   TEXT    NOT NULL,
    updated_at TEXT    NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, controller)
);
