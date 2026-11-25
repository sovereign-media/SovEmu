---
paths:
  - "SovEmu.Backend/SovEmu.Protocol/**"
  - "SovEmu.Backend/SovEmu.Server/schema.graphql"
  - "SovEmu.Backend/SovEmu.Server/src/graphql/**"
  - "SovEmu.Backend/SovEmu.Server/src/controllers.rs"
  - "SovEmu.Web/src/lib/protocol.ts"
  - "SovEmu.Web/src/lib/controllers.ts"
  - "SovEmu.Web/src/lib/queries.ts"
  - "SovEmu.Web/src/graphql-env.d.ts"
  - "SovEmu.Android/core/**"
  - "SovEmu.iOS/Packages/SovEmuKit/**"
  - "docs/CONTROLLERS.md"
---

# What every client shares

Three things exist once per client (web, Android, iOS) and once on the server. A change to any of
them changes all four together: a PR in each repository it touches.

## The GraphQL schema

`SovEmu.Backend/SovEmu.Server/schema.graphql` is checked in; a test fails when it drifts
(`UPDATE_SCHEMA=1 cargo test -p sovemu-server schema_file`). Then:

- **Web**: `npm run schema` in `SovEmu.Web` writes `src/graphql-env.d.ts` (committed); the build
  checks every document against it.
- **Android**: `./gradlew :core:syncSchema` copies it to `core/src/main/graphql/schema.graphqls`;
  Apollo checks every operation.
- **iOS**: `scripts/sync-schema.sh` copies it to `SovEmuKit/GraphQL/schema.graphqls` and runs
  Apollo iOS codegen, which checks every operation in `GraphQL/operations.graphql`.

## The input wire format

`SovEmu.Protocol/src/input.rs`, `SovEmu.Web/src/lib/protocol.ts`, Android's `core/.../Protocol.kt`
and iOS's `SovEmuKit/Protocol.swift`: input `0x01`, hotkeys `0x02`, analog `0x03`, stats `0x04`,
ack `0x81`, pipeline `0x82`, each tested against the same byte vectors. Bump `INPUT_VERSION` if an
old client would misread the new format.

## Controllers

Described once, in [docs/CONTROLLERS.md](../../docs/CONTROLLERS.md): IDs (known models such as
`xbox` and `dualsense`, else `pad:<vendor>:<product>`), the positional input names, labels,
defaults, hotkeys and players. The web client (`src/lib/controllers.ts`), Android
(`Controllers.kt`) and iOS (`Controllers.swift`) each implement it, and the server
(`controllers.rs`) stores each user's mappings per controller ID and checks them against it.
Change the document and every implementation together.

## Admin screens are web-only

The apps get schema changes, never admin screens (jobs, resources, server status).
