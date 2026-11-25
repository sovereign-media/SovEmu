---
paths:
  - "SovEmu.Web/**"
---

# Web client (SovEmu.Web): architecture

How the web client is put together. Code style is [react.md](react.md); tests are
[web-tests.md](web-tests.md).

- **Vite + React + TanStack Router, one static page.** `npm run build` (`tsc && gql.tada check
  && vite build`) writes `dist/` (`index.html` and fingerprinted `assets/`), which the Rust
  server serves next to `/graphql` (in the image, from `/opt/sovemu/web`), answering every other
  page path with `index.html`. No SSR, no Node at run time: anything dynamic is client-side
  TypeScript talking to the GraphQL API and WebRTC. There is no `StrictMode`, on purpose
  (`main.tsx`): its doubled effects would start a game session twice.
- **Same origin.** The page and the API share an origin, so use relative paths (`/graphql`, the
  `/media/…` URLs the API returns), never an absolute URL or a configured host. `npm run dev`
  proxies `/graphql`, `/media`, `/oauth` and `/.well-known` to `SOVEMU_API` (default
  `127.0.0.1:8080`).

## Data

- **GraphQL, typed by gql.tada** (`src/lib/gql.ts`). Every operation is written once in
  `src/lib/queries.ts` with `graphql(...)`; TypeScript infers its result and variables from
  `src/graphql-env.d.ts`, which `npm run schema` writes from the server's `schema.graphql`
  (committed, never edited by hand; CI and the pre-push hook run `schema:check`). A fragment
  spread passes its fragment: `graphql(\`...\`, [GameFields])`. Types are `ResultOf<typeof Doc>`,
  enums `Enum<"SettingSource">`. `gql.tada check` (in the build) is what fails on a field the
  schema lacks.
- **One transport.** `src/lib/graphql.ts` is the only module that calls `fetch`: it adds the
  bearer token, turns errors into `ApiError` with the server's `code`, and sends
  `UNAUTHENTICATED` to sign-in. Screens call `api.*` (`src/lib/api.ts`), never `fetch`.
- **Reads go through TanStack Query** (`src/lib/data.ts`): one key and fetch per read in `q`,
  used with `useQuery` (or `queryClient.fetchQuery` in imperative code such as the player), never
  a `useEffect` that calls `api.*` into state. Its loading, error and cached states are the
  screen's. A read made signed out (the sign-in screen's) stays a plain `api.*` call: it is not
  the signed-in user's to cache.
- **Writes** are `useMutation` or an `await api.*` in the handler, then mark what they made stale
  with `invalidate([...key])` (a game's reads sit under `["game", id]`), or put the server's
  answer in place with `setQueryData`. Feedback is a toast (`useToast()`).

## Screens

- **`src/lib/screens.ts` is the one list of screen paths** (`SCREEN_PATHS`), with a trailing
  slash (`/system/`, as links, bookmarks and the sign-in callback `/auth/callback/` expect).
  `src/router.tsx` gives each a title, a lazily loaded component and the URL parameters it keeps
  up to date itself, typed so neither can have a path the other lacks; the admin menu links only
  to these, and `e2e/routes.e2e.ts` opens every one by URL.
- **A screen is mounted afresh when its URL changes** (but for its own parameters), reads what it
  needs from the URL as it mounts, and cleans up after itself on unmount (listeners, timers,
  streams, a game session): the page no longer unloads between screens. Move between screens
  with `go()` (`src/lib/go.ts`) or a plain `<a href>` (followed by the router,
  `src/lib/links.ts`); never set `location.href` for an in-app screen.
- **The admin menu is one list**, `src/lib/adminNav.ts`: the admin pages in sections (each with a
  label and an icon), a submenu each in the gear's menu (a section with one page is that page),
  and headings in one list on a phone, where a submenu would open off the screen. A new admin
  page is a path in `screens.ts`, an entry in `adminNav.ts`, a route in `router.tsx`, and a
  component wrapped in `components/admin/AdminPage.tsx` (which titles it and sends a non-admin
  home).

## Structure

- **Screens and components** live in `src/components/`, in a folder per area (`home/`, `play/`,
  `browse/`, `search/`, `admin/`); a screen is the area's default export, everything else a named
  export. The frames are `Shell.tsx` and `admin/AdminPage.tsx`.
- **Controls are Radix UI** (the `radix-ui` package), styled once in `src/components/ui/`
  (`primitives.tsx`: dialog, confirm, select, progress, segmented, menu, tooltip, scroll area,
  pill, field; `button.tsx`; `toast.tsx`; class names joined with `cn()`). Reuse them; never
  hand-roll a control Radix has, and never use `prompt()`/`confirm()`. Icons are `lucide-react`.
- **Plain TypeScript (no React) in `src/lib/`**: `protocol.ts` (wire format), `input.ts`
  (keyboard and gamepad), `session.ts` (WebRTC), `stats.ts`, `controllers.ts` and the focus
  logic. Stateful logic is a class there (`Session`, `InputState`, `MenuStack`); keep logic in
  `src/lib/` where Vitest can test it, and let components only wire it to the DOM.
- **Tailwind v4 is CSS-first.** Theme tokens live in `@theme` in `src/styles/global.css`; there
  is no `tailwind.config.js`. Style with utility classes and those tokens, not new CSS files.

## Console behaviour

- **Pad and keyboard first.** Console-style screens are driven by `listenNav` (`src/lib/nav.ts`)
  with the focus logic in `src/lib/` (`home.ts`, `menu.ts`); Radix supplies the surfaces. Where
  both would see a key (Escape, Enter), make sure only one acts.
- **Latency first.** Receivers ask for `jitterBufferTarget = 0`; the input channel is unordered
  with no retransmits and carries full state on every change plus a 100 ms heartbeat; video is
  scaled with `image-rendering: pixelated` (the `.pixelated` class), never smoothed.
- The wire format and controllers are shared with the other clients: see
  [contracts.md](contracts.md).

## Before a PR

`npm run lint` (`biome ci`), `npm test` and `npm run build` from `SovEmu.Web/`; `npm run e2e`
for a change a user would see. The pre-push hook runs the first three and `schema:check`.
