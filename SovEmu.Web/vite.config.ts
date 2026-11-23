/// <reference types="vitest/config" />
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { thirdPartyLicenses } from "./scripts/thirdPartyLicenses";

// A single-page app: `vite build` writes dist/ (index.html and its assets), which the SovEmu Rust
// server serves alongside its API (in the container, from /opt/sovemu/web), answering every
// other page path with index.html so the router can take it. Nothing here runs server-side.
//
// For `npm run dev`, the API (/graphql), files (/media) and sign-in (/oauth, discovery) are
// proxied to a locally running sovemu-server (SOVEMU_API, default 127.0.0.1:8080).
export default defineConfig({
  // The packages in the bundle and their licences, beside index.html (the About page's list).
  plugins: [react(), tailwindcss(), thirdPartyLicenses()],
  // Unit tests (Vitest): Intl warmed before any test, so its first use is not timed in one.
  test: { setupFiles: ["src/test-setup.ts"] },
  server: {
    proxy: Object.fromEntries(
      ["/graphql", "/media", "/oauth", "/.well-known"].map((path) => [
        path,
        process.env.SOVEMU_API ?? "http://127.0.0.1:8080",
      ]),
    ),
  },
});
