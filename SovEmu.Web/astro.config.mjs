// @ts-check

import react from "@astrojs/react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "astro/config";

// A static site: `astro build` writes dist/, which the SovEmu Rust server serves alongside its
// API (in the container, from /opt/sovemu/web). Nothing here runs server-side: pages are
// React islands (Radix UI primitives, Lucide icons) rendered in the browser.
//
// For `npm run dev`, the API (/graphql), files (/media) and sign-in (/oauth, discovery) are
// proxied to a locally running sovemu-server (SOVEMU_API, default 127.0.0.1:8080).
export default defineConfig({
  output: "static",
  integrations: [react()],
  vite: {
    plugins: [tailwindcss()],
    server: {
      proxy: Object.fromEntries(
        ["/graphql", "/media", "/oauth", "/.well-known"].map((path) => [
          path,
          process.env.SOVEMU_API ?? "http://127.0.0.1:8080",
        ]),
      ),
    },
  },
});
