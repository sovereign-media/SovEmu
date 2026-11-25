// Renders the ESRB age-rating badges (the SVGs beside this file: SovEmu's own design, the
// rating's code over its ages, not the ESRB's icons) to WebP images that ship with each client:
// this web client (src/assets/esrb/), and copies for the Android and iOS apps. 240×360 pixels
// (6× the 40×60 design, 2:3 as the ESRB's own icons), transparent outside the rounded frame.
//
//   npm run esrb-badges                 → src/assets/esrb/esrb_<rating>.webp
//   npm run esrb-badges -- <folder>     → <folder>/esrb_<rating>.webp
//
// Chromium (Playwright's, already here for the end-to-end tests) draws each SVG on a canvas and
// encodes it, so no image tools are needed. Run it again after changing an SVG, and copy the
// results into the apps.

import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
const out = resolve(process.argv[2] ?? join(here, "../../src/assets/esrb"));
const [width, height] = [240, 360];
/** Lossy WebP at this quality: flat shapes and type, so small and sharp. */
const quality = 0.92;

mkdirSync(out, { recursive: true });
const browser = await chromium.launch();
const page = await browser.newPage();
for (const file of readdirSync(here)
  .filter((f) => f.endsWith(".svg"))
  .sort()) {
  const svg = readFileSync(join(here, file), "utf8");
  const url = await page.evaluate(
    async ({ svg, width, height, quality }) => {
      const image = new Image(width, height);
      image.src = `data:image/svg+xml;base64,${btoa(svg)}`;
      await image.decode();
      const canvas = document.createElement("canvas");
      canvas.width = width;
      canvas.height = height;
      canvas.getContext("2d").drawImage(image, 0, 0, width, height);
      return canvas.toDataURL("image/webp", quality);
    },
    { svg, width, height, quality },
  );
  if (!url.startsWith("data:image/webp;base64,")) throw new Error(`${file}: this Chromium cannot encode WebP`);
  const name = `esrb_${file.replace(/\.svg$/, "")}.webp`;
  writeFileSync(join(out, name), Buffer.from(url.slice(url.indexOf(",") + 1), "base64"));
  console.log(`${name} → ${out}`);
}
await browser.close();
