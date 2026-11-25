---
paths:
  - "SovEmu.Backend/SovEmu.Server/src/main.rs"
  - "SovEmu.Backend/SovEmu.Server/src/structured.rs"
  - "SovEmu.Backend/SovEmu.Server/src/settings.rs"
  - "SovEmu.Backend/SovEmu.Server/src/configuration.rs"
  - "SovEmu.Backend/SovEmu.Server/src/graphql/settings.rs"
  - ".env.example"
  - "compose*.yaml"
---

# Settings: every setting is an environment variable

Nothing needs click-ops.

- **A new setting is a `SovEmu__<Area>__<Name>` variable on `Config`** (`main.rs`): PascalCase
  levels joined by `__`, as .NET and Sovereign name theirs, read in any letter case. It also gets
  a row in the README's table and a line in `.env.example`.
- **Lists and maps use the `__` hierarchy**: one entry per index (`SovEmu__Users__0__Username`,
  `__1`), maps by key (`SovEmu__Systems__psx__Options__<name>`), parsed in `structured.rs` and
  gathered by `environment_changes` in `main.rs`.
- **Secrets come from the environment like any other setting** (Docker or Kubernetes secrets, no
  `_FILE` files) and join `SECRETS` in `main.rs`, so the startup log and the Configuration page
  redact them.
- **Anything an admin can do in the UI also happens on its own from configuration**: accounts,
  rescans, mirror syncs, metadata lookups, per-system cores and options. UI buttons are shortcuts,
  never the only way.
- **Where the UI stores a setting** (emulator settings, `system_settings`), the environment still
  wins: the UI shows it locked, and the page offers the stored values as environment lines.
- **Tunables are settings, not constants.**
- A setting that needs more than a variable (a secret, a volume, a port) also needs the cluster's
  Helm chart, kept outside this repository, to provide it.
