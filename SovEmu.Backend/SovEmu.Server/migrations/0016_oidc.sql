-- Roles replace the admin flag: a `viewer` plays, an `admin` also runs the server.
ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'viewer'
    CHECK (role IN ('viewer', 'admin'));
UPDATE users SET role = 'admin' WHERE is_admin = 1;
ALTER TABLE users DROP COLUMN is_admin;

-- The server is an OpenID Connect provider. `auth_sessions` stays: it is now the browser's
-- sign-in at the provider (the cookie `/oauth/authorize` reads), not the API's.

-- Authorization codes (the code flow with PKCE): single use, short-lived. Only a SHA-256 of the
-- code is stored.
CREATE TABLE oauth_codes (
    code_hash      TEXT    PRIMARY KEY,
    client_id      TEXT    NOT NULL,
    user_id        INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    redirect_uri   TEXT    NOT NULL,
    code_challenge TEXT    NOT NULL,
    scope          TEXT    NOT NULL,
    nonce          TEXT,
    -- When the user signed in (Unix seconds), for the ID token's `auth_time`.
    auth_time      INTEGER NOT NULL,
    expires_at     TEXT    NOT NULL
);

-- One per sign-in of an app: its tokens hang off it, and deleting it signs that app out.
CREATE TABLE oauth_grants (
    id         INTEGER PRIMARY KEY,
    client_id  TEXT    NOT NULL,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope      TEXT    NOT NULL,
    -- When the user signed in (Unix seconds): every ID token of this sign-in repeats it.
    auth_time  INTEGER NOT NULL,
    created_at TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX oauth_grants_user ON oauth_grants(user_id);

-- Access and refresh tokens. Only a SHA-256 of each is stored. A refresh token is used once:
-- `used_at` is when it was exchanged, kept so a replay can be recognised.
CREATE TABLE oauth_tokens (
    token_hash TEXT    PRIMARY KEY,
    grant_id   INTEGER NOT NULL REFERENCES oauth_grants(id) ON DELETE CASCADE,
    kind       TEXT    NOT NULL CHECK (kind IN ('access', 'refresh')),
    expires_at TEXT    NOT NULL,
    used_at    TEXT
);
CREATE INDEX oauth_tokens_grant ON oauth_tokens(grant_id);
