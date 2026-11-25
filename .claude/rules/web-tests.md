---
paths:
  - "SovEmu.Web/src/**/*.test.ts"
  - "SovEmu.Web/src/test-setup.ts"
  - "SovEmu.Web/e2e/**"
  - "SovEmu.Web/playwright.config.ts"
  - "SovEmu.Web/vite.config.ts"
---

# Web client tests (SovEmu.Web)

Unit tests are Vitest over `src/lib/`; end-to-end tests are Playwright against a real server. The
house style for what a test says is Sovereign's (`react-tests.md`).

## Vitest

- **Colocated**: `src/lib/menu.ts` → `src/lib/menu.test.ts`, one test file per source file. No
  `__tests__/` folders.
- **Configuration is the `test` block of `vite.config.ts`**; never add a `vitest.config.ts`. Its
  only setup file, `src/test-setup.ts`, warms up `Intl`; keep it that small.
- **Node environment, no DOM.** The logic under test is plain TypeScript in `src/lib/`. A suite
  that genuinely needs a DOM opts in on line 1 with `// @vitest-environment jsdom`; never set an
  environment globally. Components are checked end to end, not with a rendering library.
- **No globals**: import `describe`, `it`, `expect`, `vi` from `"vitest"`.
- **Swap dependencies through the module's own setters** (`setNavigator()` in `go.ts`,
  `useSession()` in `graphql.ts`), not by mocking modules.

## What a test is for

**Pin the invariant, and say why it matters**, not restate the implementation. A test carries a
comment naming the failure it prevents, in the code's own terms:

```ts
it("sends the full pad state on every change, not a diff", () => {
  // The channel is unordered with no retransmits: a lost diff would leave a button held
  // forever, while the next full state (or the 100 ms heartbeat) repairs a lost packet.
  …
});
```

Prefer, in order: contract invariants stated elsewhere (the wire vectors shared with
`SovEmu.Protocol`, `docs/CONTROLLERS.md`); boundaries and guards (zero, an unknown enum value from
a newer server, a pad with no mapping); and the shapes an input really takes.

- **Guard against a vacuous test.** Where an assertion could pass for the wrong reason, assert
  the precondition too.
- **Pin reality, flag the oddity.** When behaviour is a quirk rather than a decision, assert what
  the code does and say so in a comment, so a change is a conscious one.
- **Pin a bug with `it.fails`.** A bug found whose fix is out of scope gets the correct behaviour
  as `it.fails` with a `// BUG:` comment; it turns red when fixed, and becomes a plain `it`. Flip
  it once to confirm it fails on the assertion the comment names.
- **Names read as a sentence** completing "it …" (`it("repeats a held direction after the
  delay")`), never `it("works")`. `describe` per exported function or per behaviour.
- **Fixtures are small factories with overrides** (`const pad = (over: Partial<Pad> = {}) =>
  ({ …DEFAULT, ...over })`), with fixed values, not random ones.
- **Don't test** generated code (`graphql-env.d.ts`), `queries.ts` document literals, the router
  table, or a hook whose whole body is a `useQuery` call: that asserts TanStack Query works.
- Tests are type-checked and linted like any source: an unused import fails the build.

## Playwright (`e2e/`)

- `*.e2e.ts` files (Vitest owns `*.test.ts`) run in Chromium against a real `sovemu-server` that
  `e2e/server.mjs` builds and starts on a fresh data folder (`test-results/e2e-data`) with
  stand-in games (a few bytes named like ROMs, never real ones) and an admin. `auth.setup.ts`
  signs in once through the real sign-in, and every test starts signed in.
- **Find elements as a user would**: by role and label (`getByRole("button", { name: "Rescan"
  })`, `getByLabel("Username")`). Where no role fits, a `data-*` attribute the component sets for
  the purpose (`[data-game-card]`); never a CSS class.
- Every screen in `SCREEN_PATHS` must open by its URL alone (`routes.e2e.ts`). A screen or flow
  worth checking by eye gets a test here, and screenshots are taken with Playwright, not by hand.
- `npm run e2e` (builds everything), `npm run e2e:ui` (UI mode); `SOVEMU_E2E_URL` points the tests
  at a server already running. First time: `npx playwright install chromium`.
- Wait for what you check (`expect(...).toBeVisible()`, a response), never a fixed timeout.
