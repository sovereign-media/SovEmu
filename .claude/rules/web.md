---
paths:
  - "SovEmu.Web/**"
---

# Web client (SovEmu.Web)

- **Astro + Tailwind v4, static output only.** `astro build` writes `dist/`, which the Rust
  server serves next to `/api/*` (in the image, from `/opt/sovemu/web`). No SSR adapter, no
  Astro server endpoints, no Node at runtime. Anything dynamic is client-side TypeScript talking
  to `/api/*` and WebRTC.
- **Same origin.** The page and the API share an origin, so fetch relative paths (`/api/offer`),
  never an absolute URL or a configured host. `npm run dev` proxies `/api` to a local server.
- **Tailwind v4 is CSS-first.** Theme tokens live in `@theme` in `src/styles/global.css`; there
  is no `tailwind.config.js`. A component `<style>` that uses `@apply` needs
  `@reference "../styles/global.css";` first.
- **React islands, Radix UI, Lucide.** A page is a thin `.astro` file mounting one React
  component with `client:only="react"` (no SSR: it still builds to static files). Controls come
  from Radix UI (`radix-ui`), styled once in `src/components/ui/` (dialogs, confirm, select,
  progress, dropdown menu, tooltip, scroll area, toast, `Button`); never hand-roll a
  control Radix has, and never use `prompt()`/`confirm()`. Icons are `lucide-react`.
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
- Before a PR: `npm test` and `npm run build` (which runs `astro check`) from `SovEmu.Web/`.
