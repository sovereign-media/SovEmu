-- Controller mappings per system as well as per controller model: `system` is a system id, or
-- '' for every system (the mappings saved before this). SQLite cannot change a primary key, so
-- both tables are rebuilt.
CREATE TABLE controller_mappings_new (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    controller TEXT    NOT NULL,
    system     TEXT    NOT NULL DEFAULT '',
    name       TEXT    NOT NULL DEFAULT '',
    bindings   TEXT    NOT NULL,
    updated_at TEXT    NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, controller, system)
);
INSERT INTO controller_mappings_new (user_id, controller, system, name, bindings, updated_at)
    SELECT user_id, controller, '', name, bindings, updated_at FROM controller_mappings;
DROP TABLE controller_mappings;
ALTER TABLE controller_mappings_new RENAME TO controller_mappings;

CREATE TABLE controller_defaults_new (
    controller TEXT NOT NULL,
    system     TEXT NOT NULL DEFAULT '',
    name       TEXT NOT NULL DEFAULT '',
    bindings   TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (controller, system)
);
INSERT INTO controller_defaults_new (controller, system, name, bindings, updated_at)
    SELECT controller, '', name, bindings, updated_at FROM controller_defaults;
DROP TABLE controller_defaults;
ALTER TABLE controller_defaults_new RENAME TO controller_defaults;
