// @ts-check
import { defineConfig } from "astro/config";
import tailwindcss from "@tailwindcss/vite";
import react from "@astrojs/react";

// A static site: `astro build` writes dist/, which the SovEmu Rust server serves alongside its
// /api routes (in the container, from /opt/sovemu/web). Nothing here runs server-side: pages are
// React islands (Radix UI primitives, Lucide icons) rendered in the browser.
//
// For `npm run dev`, /api is proxied to a locally running server, so start one with e.g.
//   sovemu-worker serve --core ... --http 127.0.0.1:8080
export default defineConfig({
  output: "static",
  integrations: [react()],
  vite: {
    plugins: [tailwindcss()],
    server: {
      proxy: { "/api": process.env.SOVEMU_API ?? "http://127.0.0.1:8080" },
    },
  },
});
