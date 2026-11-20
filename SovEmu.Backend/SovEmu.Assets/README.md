# SovEmu.Assets (`sovemu-assets`)

Signed file URLs, the same scheme Sovereign uses. Every file URL the API hands a client
(artwork, videos, save screenshots, decorations) carries `Sov-UserId`, `Sov-Expiry` and
`Sov-Signature`: a hex HMAC-SHA256 over `<user id>:<decoded path>:<expiry>`.

- `AssetSigner::sign(user, url)` signs a URL. The expiry is rounded down to the hour, then
  `SOVEMU_ASSET_URL_HOURS` is added, so the same file's URL stays the same for up to an hour and
  caches keep working.
- `AssetSigner::check(path, query)` returns the user the URL was signed for, or why it was
  refused (`Unsigned`, `Forged`, `Expired`).
- `load_key` returns `SOVEMU_ASSET_KEY`, or else the key kept in `<data>/server.json`, which it
  generates on first run (owner-only). `load_secret` is the same for any server secret kept
  there: the server's token signing key (`SOVEMU_TOKEN_KEY`) is the other.

The server's `/media/…` middleware (`SovEmu.Server/src/assets.rs`) calls `check`, makes sure the
user still exists, and serves the file as them. This crate knows nothing about HTTP or the
database.

```bash
cargo test -p sovemu-assets
```
