-- The activity log (`activity.rs`): who did what, and when. Sign-ins and refusals, games
-- started and ended, and what admins changed. The admin Activity page reads it newest first,
-- by user and by kind; maintenance deletes rows older than SOVEMU_ACTIVITY_HISTORY_DAYS.
CREATE TABLE activity (
    id       INTEGER PRIMARY KEY,
    -- Unix seconds.
    at       INTEGER NOT NULL,
    -- The account that did it: kept when the account is deleted (the name stays beside it).
    -- Null for a refused sign-in.
    user_id  INTEGER,
    -- The name as given: for a refused sign-in, the name that was tried.
    username TEXT,
    -- `signed_in`, `sign_in_failed`, `game_started`, `role_changed`, … (`activity::Kind`), and
    -- the page's filter it falls under: `sign_in`, `play` or `admin`.
    kind     TEXT NOT NULL,
    category TEXT NOT NULL,
    -- What it was about (a game, an account, a job) and more about it (a role, an end reason).
    -- Never a password, a token or a secret.
    subject  TEXT,
    detail   TEXT,
    -- The client's address, where there was a request.
    address  TEXT
);

-- The page: newest first, all of it, by kind, or by user; maintenance prunes by time.
CREATE INDEX activity_by_time ON activity (at, id);
CREATE INDEX activity_by_category ON activity (category, at, id);
CREATE INDEX activity_by_user ON activity (user_id, at, id);
