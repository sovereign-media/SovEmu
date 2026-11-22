-- The container's CPU and memory use, one row per resource per minute (`resources/mod.rs`): the
-- minute's average and highest figure, as a share (0–100) of the allowed cores or the memory
-- limit. The admin Resources page charts them; maintenance deletes rows older than
-- SOVEMU_RESOURCE_HISTORY_DAYS.
CREATE TABLE resource_samples (
    id       INTEGER PRIMARY KEY,
    -- `cpu` or `memory`.
    resource TEXT NOT NULL,
    -- Unix seconds, the end of the minute: charts bucket by it.
    at       INTEGER NOT NULL,
    average  REAL NOT NULL,
    max      REAL NOT NULL
);

-- Charts read a window by time; maintenance prunes by it.
CREATE INDEX resource_samples_by_time ON resource_samples (at);
