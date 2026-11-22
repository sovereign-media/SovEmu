---
paths:
  - "SovEmu.Web/**"
---

# Web client (SovEmu.Web)

- **Vite + React + TanStack Router, one static page.** `vite build` writes `dist/`
  (`index.html` and fingerprinted `assets/`), which the Rust server serves next to `/graphql` (in
  the image, from `/opt/sovemu/web`), answering every other page path with `index.html`. No SSR,
  no Node at runtime. Anything dynamic is client-side TypeScript talking to the GraphQL API and
  WebRTC.
- **Same origin.** The page and the API share an origin, so use relative paths (`/graphql`,
  the `/media/…` URLs the API returns), never an absolute URL or a configured host.
  `npm run dev` proxies `/graphql` and `/media` to a local server.
- **GraphQL, typed by gql.tada** (`src/lib/gql.ts`). Every operation is written once in
  `src/lib/queries.ts` with `graphql(...)`; TypeScript infers its result and variables from
  `src/graphql-env.d.ts`, which `npm run schema` writes from the server's `schema.graphql`
  (committed; CI and the pre-push hook run `schema:check`). A fragment spread passes its
  fragment: `graphql(\`...\`, [GameFields])`. Types are `ResultOf<typeof Doc>`, enums
  `Enum<"SettingSource">`. `npm run build` runs `gql.tada check`, which fails on a field the
  schema lacks (`tsc` alone does not). Pages
  call `api.*` in `src/lib/api.ts`, never `fetch`. Errors are `ApiError` with the server's
  `code`; `UNAUTHENTICATED` sends the page to sign-in.
- **Reads go through TanStack Query** (`src/lib/data.ts`): one key and fetch per read in `q`,
  used with `useQuery` (or `queryClient.fetchQuery` in imperative code such as the player),
  never a `useEffect` that calls `api.*` into state. Its loading, error and cached states are
  the screen's. A change marks what it made stale with `invalidate([...key])` (a game's reads
  sit under `["game", id]`), or puts the server's answer in place with `setQueryData`. A
  read made signed out (the sign-in screen's) stays a plain `api.*` call: it is not the
  signed-in user's to cache.
- **Tailwind v4 is CSS-first.** Theme tokens live in `@theme` in `src/styles/global.css`; there
  is no `tailwind.config.js`. A component `<style>` that uses `@apply` needs
  `@reference "../styles/global.css";` first.
- **Screens are routes** in `src/router.tsx`: a path (trailing slash: `/system/`, as links,
  bookmarks and the sign-in callback `/auth/callback/` expect), a title, a component (loaded on
  first visit) and the URL parameters it keeps up to date itself. A screen is mounted afresh
  when the URL changes, reads what it needs from the URL as it mounts, and must clean up after
  itself on unmount (listeners, timers, streams, a game session): the page no longer unloads
  between screens. Move between screens with `go()` (`src/lib/go.ts`) or a plain `<a href>`
  (followed by the router, `src/lib/links.ts`); never set `location.href` for an in-app screen.
- **Radix UI, Lucide.** Controls come
  from Radix UI (`radix-ui`), styled once in `src/components/ui/` (dialogs, confirm, select,
  progress, dropdown menu, tooltip, scroll area, toast, `Button`); never hand-roll a
  control Radix has, and never use `prompt()`/`confirm()`. Icons are `lucide-react`.
- **The admin menu is one list**, `src/lib/adminNav.ts`, as Sovereign's `adminNavItems.ts`: the
  gear's groups and their pages, which also title each page (`adminDocumentTitle`). A new admin
  page is an entry there, a route in `src/router.tsx`, and a component that sends a non-admin
  home.
- **Pad and keyboard first.** Console-style screens are driven by `listenNav` (`src/lib/nav.ts`)
  with the focus logic in `src/lib/` (`home.ts`, `menu.ts`); Radix supplies the surfaces. Where
  both would see a key (Escape, Enter), make sure only one acts.
- **Structure.** Pages in `src/pages/`, layouts in `src/layouts/`, React components in
  `src/components/`, and plain TypeScript (no framework) in `src/lib/`: `protocol.ts` (wire
  format), `input.ts` (keyboard and gamepad), `session.ts` (WebRTC), `stats.ts`. Keep logic in
  `src/lib/` where Vitest can test it; components only wire it to the DOM.
- **The wire format mirrors SovEmu.Protocol.** `src/lib/protocol.ts` and
  `SovEmu.Backend/SovEmu.Protocol/src/input.rs` are tested against the same byte vectors; change
  both together.
- **Latency first.** Receivers ask for `jitterBufferTarget = 0`; the input channel is unordered
  with no retransmits and carries full state on every change plus a 100 ms heartbeat; video is
  scaled with `image-rendering: pixelated` (the `.pixelated` class), never smoothed.
- **Biome lints and formats** (`biome.jsonc`): `npm run format` rewrites, `npm run lint`
  (`biome ci`) checks, in CI and the pre-push hook. Warnings (effect dependencies, some a11y
  rules for the pad-driven screens) are worth a look but do not fail; errors do. Never edit
  `src/graphql-env.d.ts` by hand: `npm run schema` writes it.
- **End-to-end tests are Playwright** (`e2e/`, `playwright.config.ts`): `*.e2e.ts` files (Vitest
  owns `*.test.ts`) run in Chromium against a real `sovemu-server` that `e2e/server.mjs` builds
  and starts on a fresh data folder (`test-results/e2e-data`) with stand-in games (a few bytes
  named like ROMs, never real ones) and an admin; `auth.setup.ts` signs in once through the real
  sign-in and every test starts signed in. Find elements by role and label
  (`getByRole("button", { name: "Rescan" })`, `getByLabel("Username")`), never by CSS class.
  A screen or flow worth checking by eye gets a test here. `SOVEMU_E2E_URL` runs them against a
  server already running; `npm run e2e:ui` opens Playwright's UI.
- Before a PR: `npm run lint`, `npm test` and `npm run build` (which runs `tsc` first) from
  `SovEmu.Web/`.
