-- Every run of a background job (`jobs.rs`): a library scan, a mirror sync, a game-information
-- lookup, database upkeep. A row is written as a run starts (`running`) and finished when it
-- ends; one still `running` at startup was cut short by the server stopping (`interrupted`).
-- Maintenance deletes rows older than SOVEMU_JOB_HISTORY_DAYS.
CREATE TABLE job_runs (
    id          INTEGER PRIMARY KEY,
    job         TEXT NOT NULL,
    -- `startup`, `schedule` or `admin`.
    trigger     TEXT NOT NULL,
    -- `running`, `succeeded`, `failed`, `cancelled` or `interrupted`.
    outcome     TEXT NOT NULL DEFAULT 'running',
    started_at  TEXT NOT NULL DEFAULT (datetime('now')),
    finished_at TEXT,
    duration_ms INTEGER,
    -- What the run did ("1,204 games: 3 added, 0 removed") or why it failed.
    message     TEXT
);

-- The admin Jobs page: each job's latest runs.
CREATE INDEX job_runs_by_job ON job_runs (job, id);
-- Maintenance's pruning.
CREATE INDEX job_runs_by_start ON job_runs (started_at);
