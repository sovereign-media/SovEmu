// Renders the ESRB's rating icons (the SVGs beside this file, from Wikimedia Commons: see
// README.md) to the PNGs the server ships in its built-in decoration set
// (`SovEmu.Library/assets/ratings/esrb/`, written to `<decorations>/sovemu/ratings/esrb/` with the
// rest of the set; an admin replaces one by putting their own at that path in another decorations
// folder). 240 pixels wide, as tall as each icon's own shape (2:3), transparent where the icon is.
//
//   npm run esrb-badges                 → SovEmu.Library/assets/ratings/esrb/<rating>.png
//   npm run esrb-badges -- <folder>     → <folder>/<rating>.png
//
// Chromium (Playwright's, already here for the end-to-end tests) draws each SVG on a canvas and
// encodes it, so no image tools are needed.

import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
const out = resolve(process.argv[2] ?? join(here, "../../../SovEmu.Backend/SovEmu.Library/assets/ratings/esrb"));
const width = 240;

mkdirSync(out, { recursive: true });
const browser = await chromium.launch();
const page = await browser.newPage();
for (const file of readdirSync(here)
  .filter((f) => f.endsWith(".svg"))
  .sort()) {
  const svg = readFileSync(join(here, file), "utf8");
  const [, , w, h] = (svg.match(/viewBox="([^"]+)"/)?.[1] ?? "0 0 2 3").split(/[\s,]+/).map(Number);
  const height = Math.round((width * h) / w);
  const url = await page.evaluate(
    async ({ svg, width, height }) => {
      const image = new Image(width, height);
      image.src = `data:image/svg+xml;base64,${btoa(svg)}`;
      await image.decode();
      const canvas = document.createElement("canvas");
      canvas.width = width;
      canvas.height = height;
      canvas.getContext("2d").drawImage(image, 0, 0, width, height);
      return canvas.toDataURL("image/png");
    },
    { svg, width, height },
  );
  const name = file.replace(/\.svg$/, ".png");
  writeFileSync(join(out, name), Buffer.from(url.slice(url.indexOf(",") + 1), "base64"));
  console.log(`${name} → ${out}`);
}
await browser.close();
