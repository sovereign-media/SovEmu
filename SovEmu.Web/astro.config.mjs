// @ts-check
import { defineConfig } from "astro/config";
import tailwindcss from "@tailwindcss/vite";

// A static site: `astro build` writes dist/, which the SovEmu Rust server serves alongside its
// /api routes (in the container, from /opt/sovemu/web). Nothing here runs server-side.
//
// For `npm run dev`, /api is proxied to a locally running server, so start one with e.g.
//   sovemu-worker serve --core ... --http 127.0.0.1:8080
export default defineConfig({
  output: "static",
  vite: {
    plugins: [tailwindcss()],
    server: {
      proxy: { "/api": process.env.SOVEMU_API ?? "http://127.0.0.1:8080" },
    },
  },
});
