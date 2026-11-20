-- Roles replace the admin flag: a `viewer` plays, an `admin` also runs the server.
ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'viewer'
    CHECK (role IN ('viewer', 'admin'));
UPDATE users SET role = 'admin' WHERE is_admin = 1;
ALTER TABLE users DROP COLUMN is_admin;

-- The server is an OpenID Connect provider. `auth_sessions` stays: it is now the browser's
-- sign-in at the provider (the cookie `/oauth/authorize` reads), not the API's.

-- The authorization server's storage (oauth-as's `Storage`, `SovEmu.Identity/src/store.rs`).
-- Each record is kept whole as JSON in `record`, beside the columns it is looked up, cascaded
-- and swept by. Instants are Unix nanoseconds. Access tokens, refresh tokens and authorization
-- codes are keyed by the SHA-256 (hex) of the credential, and the credential is left out of
-- `record`. `subject` is the user id as text: what ending every sign-in of a user deletes by.
-- There are no foreign keys: oauth-as decides what a subject or a client is, and its
-- revocations do the cascading.

-- Registered clients (applications that may sign in).
CREATE TABLE oauth_clients (
    client_id TEXT PRIMARY KEY,
    record    TEXT NOT NULL
);

-- RFC 8628 device grants. `user_code` is the normalized code a person types, unique so that one
-- code never answers for two devices; `subject` is set once the grant is approved.
CREATE TABLE oauth_device_grants (
    device_code TEXT    PRIMARY KEY,
    user_code   TEXT    NOT NULL UNIQUE,
    client_id   TEXT    NOT NULL,
    subject     TEXT,
    state       TEXT    NOT NULL,
    expires_at  INTEGER NOT NULL,
    record      TEXT    NOT NULL
);
CREATE INDEX oauth_device_grants_client ON oauth_device_grants(client_id);
CREATE INDEX oauth_device_grants_subject ON oauth_device_grants(subject);
CREATE INDEX oauth_device_grants_expiry ON oauth_device_grants(expires_at);

-- Authorization codes, in either state (a consumed code is kept until it expires so a replay is
-- recognised). `state_digest` is the SHA-256 of the state, what a compare-and-swap compares.
CREATE TABLE oauth_authorization_codes (
    code_hash    TEXT    PRIMARY KEY,
    client_id    TEXT    NOT NULL,
    subject      TEXT    NOT NULL,
    state_digest TEXT    NOT NULL,
    expires_at   INTEGER NOT NULL,
    record       TEXT    NOT NULL
);
CREATE INDEX oauth_authorization_codes_client ON oauth_authorization_codes(client_id);
CREATE INDEX oauth_authorization_codes_subject ON oauth_authorization_codes(subject);
CREATE INDEX oauth_authorization_codes_expiry ON oauth_authorization_codes(expires_at);

-- Access tokens. `family_id` is the refresh chain the token was minted along, if any.
CREATE TABLE oauth_access_tokens (
    token_hash TEXT    PRIMARY KEY,
    client_id  TEXT    NOT NULL,
    subject    TEXT,
    family_id  TEXT,
    expires_at INTEGER NOT NULL,
    record     TEXT    NOT NULL
);
CREATE INDEX oauth_access_tokens_client ON oauth_access_tokens(client_id);
CREATE INDEX oauth_access_tokens_subject ON oauth_access_tokens(subject);
CREATE INDEX oauth_access_tokens_family ON oauth_access_tokens(family_id);
CREATE INDEX oauth_access_tokens_expiry ON oauth_access_tokens(expires_at);

-- Refresh tokens, live and spent (a spent one is kept until `expires_at` so reuse is detected;
-- a chain with no absolute lifetime has none).
CREATE TABLE oauth_refresh_tokens (
    token_hash TEXT    PRIMARY KEY,
    client_id  TEXT    NOT NULL,
    subject    TEXT,
    family_id  TEXT    NOT NULL,
    expires_at INTEGER,
    record     TEXT    NOT NULL
);
CREATE INDEX oauth_refresh_tokens_client ON oauth_refresh_tokens(client_id);
CREATE INDEX oauth_refresh_tokens_subject ON oauth_refresh_tokens(subject);
CREATE INDEX oauth_refresh_tokens_family ON oauth_refresh_tokens(family_id);
CREATE INDEX oauth_refresh_tokens_expiry ON oauth_refresh_tokens(expires_at);

-- What a revocation removed, so that an issuance already in flight cannot write it back: a
-- deleted client, a revoked refresh family, a withdrawn consent (`scope` the client, `subject`
-- the user) or every sign-in of one user (`scope` the user id). Swept at `until`.
CREATE TABLE oauth_revocation_barriers (
    kind        TEXT    NOT NULL CHECK (kind IN ('client', 'family', 'consent', 'subject')),
    scope       TEXT    NOT NULL CHECK (scope <> ''),
    subject     TEXT    NOT NULL DEFAULT '',
    recorded_at INTEGER NOT NULL,
    until       INTEGER NOT NULL,
    PRIMARY KEY (kind, scope, subject)
);
CREATE INDEX oauth_revocation_barriers_until ON oauth_revocation_barriers(until);
