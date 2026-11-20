# SovEmu.Identity (`sovemu-identity`)

Accounts and sign-in. The server (`SovEmu.Server`) mounts its routes and asks it who a request
is. It is handed the server's database pool; its tables are the server's migrations. It knows
nothing about GraphQL.

## What is whose

The OAuth protocol is [`oauth-as`](https://crates.io/crates/oauth-as)'s, an embeddable OAuth 2.1
authorization server (stable 1.x; 1.0.0 is OpenID Certified to the FAPI 2.0 profile). It covers:

- the authorization code flow, with PKCE required (S256 only) and exact redirect URIs;
- the device grant (RFC 8628) and its `/device` verification page;
- refresh rotation with reuse detection;
- revocation, which cascades to the whole sign-in;
- the RFC 8414 metadata and the `iss` response parameter;
- rate limiting of device-code entry.

Do not reimplement any of that here.

| Module | What |
|---|---|
| `accounts` | Users: argon2 passwords, `Role` (`viewer`, `admin`), creating, declaring from the environment, pruning |
| `throttle` | Failed sign-ins per username and per client address |
| `sessions` | The browser's sign-in at the server: an HttpOnly cookie, only its SHA-256 stored (`auth_sessions`) |
| `store` | `SqliteStorage`, oauth-as's `Storage` over SQLite (the `oauth_*` tables), plus `revoke_subject` (sign a user out everywhere). Tested with oauth-as's conformance harness |
| `keys` | The ES256 key ID tokens are signed with (oauth-as's `EcdsaP256Key`), derived from `SOVEMU_TOKEN_KEY` |
| `provider` | What SovEmu adds around oauth-as (below) |

What `provider` adds:

- **The front of `/oauth/authorize` and `/device`.** oauth-as has no sessions, so this layer reads
  the browser's cookie. It sends a signed-out browser to `/login/`, and otherwise names the user
  to oauth-as in a header it strips from every incoming request.
- **OpenID Connect:**
  - `/.well-known/openid-configuration`: oauth-as's metadata plus the OIDC fields;
  - an ID token added to the token endpoint's answer when `openid` was granted, carrying the
    `nonce` from the authorization request;
  - `/oauth/jwks`, `/oauth/userinfo`, and `/oauth/logout` (end session).
- **`POST /oauth/signin`:** the sign-in page's form, which refuses posts from other origins.
- **`POST /oauth/password`:** the apps' password fallback. OAuth 2.1 has no password grant, so
  this route checks the password and then runs an ordinary PKCE code exchange with oauth-as on the
  app's behalf. The tokens are oauth-as's own, and they refresh and revoke like any others.
  `SOVEMU_PASSWORD_SIGN_IN=false` turns it off.
- **One oauth-as server per address in `SOVEMU_PUBLIC_URL`** (an issuer is fixed per server), all
  sharing one store. A request is answered by the server for its `Host`.

## Clients

| Client | Grants | Used by |
|---|---|---|
| `sovemu-web` | authorization code (PKCE), refresh | The web client. Never a password: the browser's password goes only to the sign-in form |
| `sovemu-native` | device code, refresh; the password fallback | Android and iOS |

```bash
cargo test -p sovemu-identity
```
