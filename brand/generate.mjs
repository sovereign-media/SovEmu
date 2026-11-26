#!/usr/bin/env node
/**
 * Draws SovEmu's mark once and writes every size and format each client takes.
 *
 * The mark is Sovereign's shield outline (sovereign-shield.svg, copied from Sovereign.Design) with
 * an 8-bit S in place of Sovereign's S: 5 by 7 pixels, each a slightly rounded square. The icon is
 * the mark in white on SovEmu's blue (#4169E1, the web client's accent). The wordmark is "Sov" and
 * "Emu" in Inter Bold (fonts/, SIL OFL), outlined, so no output depends on a font being installed.
 *
 * Usage, from this folder:
 *   npm ci
 *   node generate.mjs                 # clients are this repository's submodules
 *   node generate.mjs --clients <dir> # or checkouts of them in <dir> (SovEmu.Android, ...)
 *
 * Writes:
 *   brand/sovemu-mark.svg, sovemu-icon.svg (rounded tile), sovemu-icon-square.svg (full bleed),
 *   brand/sovemu-wordmark.svg ("Sov" white, "Emu" in the accent, for dark backgrounds),
 *   brand/png/                     the icon as PNG at store and general sizes
 *   SovEmu.Web/public/             favicon.svg, apple-touch-icon.png, sovemu-mark.svg (the brand mark)
 *   SovEmu.Docs/public/            favicon.svg, apple-touch-icon.png
 *   SovEmu.Android/common/...      the launcher icon's foreground (and themed layer), the TV
 *                                  banner (mark and name), and the design system's mark
 *   SovEmu.iOS/Apps/Mobile/Assets.xcassets   the app icon and the mark
 * Every output is generated: change this file or sovereign-shield.svg, never an output.
 */
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { deflateSync } from 'node:zlib';
import { Resvg } from '@resvg/resvg-js';
import opentype from 'opentype.js';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..');
const argIndex = process.argv.indexOf('--clients');
const clients = argIndex > 0 ? resolve(process.argv[argIndex + 1]) : repo;

const BLUE = '#4169E1';
// "Emu" on the blue, where the accent itself would not show: the pale blue of the app's backgrounds.
const PALE = '#C9D4FF';
const WHITE = '#FFFFFF';
/** A coordinate as written out: two decimals at most. */
const n = (v) => Number(v.toFixed(2));

// The mark's box: the shield's, 282 by 366.
const W = 282;
const H = 366;

// Sovereign's shield ring: its outer and inner contours, even-odd, and where it sits in the box.
const shieldSvg = readFileSync(join(here, 'sovereign-shield.svg'), 'utf8');
const ring = /\sd="([^"]+)"/.exec(shieldSvg)[1];
const [, ringX, ringY] = /translate\(([-\d.]+),([-\d.]+)\)/.exec(shieldSvg).map(Number);

// The pixel S: 5 by 7 cells of 25 units, each pixel 23 units with corners rounded by 2.5,
// centred where Sovereign's S sits.
const S = ['.###.', '#...#', '#....', '.###.', '....#', '#...#', '.###.'];
const CELL = 25;
const PIXEL = 23;
const RADIUS = 2.5;
const S_X = 79.5;
const S_Y = 88.5;

function roundedSquare(x, y, size, r) {
  const e = size - 2 * r;
  return (
    `M${n(x + r)} ${n(y)}h${n(e)}a${r} ${r} 0 0 1 ${r} ${r}v${n(e)}a${r} ${r} 0 0 1 -${r} ${r}` +
    `h-${n(e)}a${r} ${r} 0 0 1 -${r} -${r}v-${n(e)}a${r} ${r} 0 0 1 ${r} -${r}z`
  );
}
const letter = S.flatMap((row, r) =>
  [...row].flatMap((ch, c) => (ch === '#' ? [roundedSquare(S_X + c * CELL, S_Y + r * CELL, PIXEL, RADIUS)] : [])),
).join('');


// The name, outlined from Inter Bold: "Sov" and "Emu" as two paths, so each takes its colour.
const inter = opentype.parse(readFileSync(join(here, 'fonts', 'Inter-Bold.ttf')).buffer);
const CAP = inter.tables.os2.sCapHeight / inter.unitsPerEm;
/**
 * `text` at `size` with its baseline at (x, y): its path data, and where the pen ends. One glyph per
 * character, kerned, without shaping: opentype.js cannot apply some of Inter's substitution
 * tables, and plain Latin letters need none.
 */
function outline(text, x, y, size) {
  const scale = size / inter.unitsPerEm;
  const glyphs = [...text].map((ch) => inter.charToGlyph(ch));
  let pen = x;
  const parts = glyphs.map((glyph, i) => {
    // Written out command by command: opentype.js's own compact form runs numbers together.
    const data = glyph
      .getPath(pen, y, size)
      .commands.map((c) => {
        if (c.type === 'Z') return 'Z';
        if (c.type === 'Q') return `Q${n(c.x1)} ${n(c.y1)} ${n(c.x)} ${n(c.y)}`;
        if (c.type === 'C') return `C${n(c.x1)} ${n(c.y1)} ${n(c.x2)} ${n(c.y2)} ${n(c.x)} ${n(c.y)}`;
        return `${c.type}${n(c.x)} ${n(c.y)}`;
      })
      .join('');
    pen += glyph.advanceWidth * scale;
    if (i + 1 < glyphs.length) pen += inter.getKerningValue(glyph, glyphs[i + 1]) * scale;
    return data;
  });
  return { d: parts.join(''), end: pen };
}
/** "Sov" and "Emu" at `size`, the baseline at (x, y): their path data, and the whole width. */
function name(x, y, size) {
  const sov = outline('Sov', x, y, size);
  const emu = outline('Emu', sov.end, y, size);
  return { sov: sov.d, emu: emu.d, width: emu.end - x };
}

/** The mark's shapes in one colour, in the 282 by 366 box. */
const markShapes = (fill) =>
  `<path fill="${fill}" fill-rule="evenodd" transform="translate(${ringX} ${ringY})" d="${ring}"/>` +
  `<path fill="${fill}" d="${letter}"/>`;

const svg = (w, h, body, title) =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">` +
  `<title>${title}</title>${body}</svg>\n`;

/** The icon: the mark centred on the blue, on a 512 square, rounded or full bleed. */
const icon = (rounded) =>
  svg(
    512,
    512,
    `<rect width="512" height="512"${rounded ? ' rx="112"' : ''} fill="${BLUE}"/>` +
      `<g transform="translate(${(512 - W) / 2} ${(512 - H) / 2})">${markShapes(WHITE)}</g>`,
    'SovEmu',
  );

const mark = (fill) => svg(W, H, markShapes(fill), 'SovEmu');

function write(path, data) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, data);
  console.log(`wrote ${path.replace(`${clients}/`, '').replace(`${repo}/`, '')}`);
}

/**
 * A PNG of an SVG at `size` pixels square. `opaque` drops the alpha channel, which App Store and
 * home-screen icons must not have: resvg renders RGBA, so the pixels are re-encoded as RGB.
 */
function png(svgText, size, opaque = false) {
  const rendered = new Resvg(svgText, { fitTo: { mode: 'width', value: size } }).render();
  if (!opaque) return rendered.asPng();
  const { width, height, pixels } = rendered;
  const raw = Buffer.alloc((width * 3 + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (width * 3 + 1)] = 0;
    for (let x = 0; x < width; x++) {
      const from = (y * width + x) * 4;
      const to = y * (width * 3 + 1) + 1 + x * 3;
      raw[to] = pixels[from];
      raw[to + 1] = pixels[from + 1];
      raw[to + 2] = pixels[from + 2];
    }
  }
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type), data]);
    const length = Buffer.alloc(4);
    length.writeUInt32BE(data.length);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(body));
    return Buffer.concat([length, body, crc]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8; // bit depth
  header[9] = 2; // colour type: RGB
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}
const CRC_TABLE = Array.from({ length: 256 }, (_, k) => {
  let c = k;
  for (let i = 0; i < 8; i++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(buffer) {
  let c = 0xffffffff;
  for (const byte of buffer) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

/** An Android VectorDrawable of the mark: `size` dp high, placed at (x, y) on a `vw` by `vh` canvas. */
function androidVector({ width, height, vw, vh, x, y, scale, background, fill, comment }) {
  return `<?xml version="1.0" encoding="utf-8"?>
<!-- ${comment}
     Generated by SovEmu's brand/generate.mjs: change that, never this file. -->
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    android:width="${width}dp"
    android:height="${height}dp"
    android:viewportWidth="${vw}"
    android:viewportHeight="${vh}">
${background ? `    <path android:fillColor="${background}" android:pathData="M0,0h${vw}v${vh}h-${vw}z" />\n` : ''}    <group android:translateX="${n(x)}" android:translateY="${n(y)}" android:scaleX="${scale}" android:scaleY="${scale}">
        <group android:translateX="${ringX}" android:translateY="${ringY}">
            <path android:fillColor="${fill}" android:fillType="evenOdd" android:pathData="${ring}" />
        </group>
        <path android:fillColor="${fill}" android:pathData="${letter}" />
    </group>
</vector>
`;
}

// The masters.
{
  // Cap height 100, with room for the round letters' overshoot above and below it.
  const size = 100 / CAP;
  const pad = 4;
  const { sov, emu, width } = name(pad, 100 + pad, size);
  write(
    join(here, 'sovemu-wordmark.svg'),
    svg(Math.ceil(width + 2 * pad), 100 + 2 * pad, `<path fill="${WHITE}" d="${sov}"/><path fill="${BLUE}" d="${emu}"/>`, 'SovEmu'),
  );
}
const roundedIcon = icon(true);
const squareIcon = icon(false);
write(join(here, 'sovemu-mark.svg'), mark(WHITE));
write(join(here, 'sovemu-icon.svg'), roundedIcon);
write(join(here, 'sovemu-icon-square.svg'), squareIcon);
// Store listings and anything else that wants a bitmap: full-bleed squares (stores and launchers
// apply their own corners), and the rounded tile for everywhere else.
for (const size of [1024, 512, 192, 180, 114]) {
  write(join(here, 'png', `sovemu-icon-square-${size}.png`), png(squareIcon, size, true));
}
for (const size of [512, 192]) write(join(here, 'png', `sovemu-icon-${size}.png`), png(roundedIcon, size));

// Web: the favicon, the home-screen icon, and the brand mark in the accent (sign-in, the corner).
const web = join(repo, 'SovEmu.Web', 'public');
write(join(web, 'favicon.svg'), roundedIcon);
write(join(web, 'apple-touch-icon.png'), png(squareIcon, 180, true));
write(join(web, 'sovemu-mark.svg'), mark(BLUE));

// Docs: the same favicon and home-screen icon.
const docs = join(clients, 'SovEmu.Docs', 'public');
write(join(docs, 'favicon.svg'), roundedIcon);
write(join(docs, 'apple-touch-icon.png'), png(squareIcon, 180, true));

// Android. The adaptive icon's foreground: the mark 54dp high, centred on the 108dp canvas, inside
// the 66dp safe zone every launcher mask keeps. It is also the themed (monochrome) layer.
const android = join(clients, 'SovEmu.Android', 'common');
const fgScale = 54 / H;
write(
  join(android, 'feature/src/main/res/drawable/ic_launcher_foreground.xml'),
  androidVector({
    width: 108,
    height: 108,
    vw: 108,
    vh: 108,
    x: (108 - W * fgScale) / 2,
    y: (108 - H * fgScale) / 2,
    scale: n(fgScale),
    fill: WHITE,
    comment: 'SovEmu\'s mark, 54dp high in the adaptive icon\'s 108dp canvas, inside its 66dp safe zone. Also the themed (monochrome) layer.',
  }),
);
// The Android TV home screen's banner, 320 by 180dp: the mark and the name on the blue, centred
// together, as Sovereign's banner shows its shield over its name.
{
  const markH = 100;
  const scale = markH / H;
  const gap = 14;
  const textSize = 48;
  const probe = name(0, 0, textSize);
  const left = (320 - (W * scale + gap + probe.width)) / 2;
  const baseline = 90 + (textSize * CAP) / 2;
  const { sov, emu } = name(left + W * scale + gap, baseline, textSize);
  const vector = androidVector({
    width: 320,
    height: 180,
    vw: 320,
    vh: 180,
    x: left,
    y: (180 - markH) / 2,
    scale: n(scale),
    background: BLUE,
    fill: WHITE,
    comment: 'The Android TV home screen\'s banner (320×180dp): SovEmu\'s mark and name on its blue.',
  }).replace(
    '</vector>',
    `    <path android:fillColor="${WHITE}" android:pathData="${sov}" />\n    <path android:fillColor="${PALE}" android:pathData="${emu}" />\n</vector>`,
  );
  write(join(android, 'feature/src/main/res/drawable/banner.xml'), vector);
}
// The design system's mark, for the brand next to "SovEmu": white, tinted where it is drawn.
write(
  join(android, 'designsystem/src/main/res/drawable/sovemu_mark.xml'),
  androidVector({
    width: 24,
    height: n((24 * H) / W),
    vw: W,
    vh: H,
    x: 0,
    y: 0,
    scale: 1,
    fill: WHITE,
    comment: 'SovEmu\'s mark, for the brand next to its name (Brand.kt tints it).',
  }),
);

// iOS: the app icon (one 1024 image, opaque; iOS applies the corners) and the mark as a template
// image, tinted where it is drawn.
const assets = join(clients, 'SovEmu.iOS', 'Apps', 'Mobile', 'Assets.xcassets');
const json = (value) => `${JSON.stringify(value, null, 2)}\n`;
write(join(assets, 'Contents.json'), json({ info: { author: 'xcode', version: 1 } }));
write(join(assets, 'AppIcon.appiconset', 'AppIcon.png'), png(squareIcon, 1024, true));
write(
  join(assets, 'AppIcon.appiconset', 'Contents.json'),
  json({
    images: [{ filename: 'AppIcon.png', idiom: 'universal', platform: 'ios', size: '1024x1024' }],
    info: { author: 'xcode', version: 1 },
  }),
);
write(join(assets, 'SovEmuMark.imageset', 'SovEmuMark.svg'), mark('#000000'));
write(
  join(assets, 'SovEmuMark.imageset', 'Contents.json'),
  json({
    images: [{ filename: 'SovEmuMark.svg', idiom: 'universal' }],
    info: { author: 'xcode', version: 1 },
    properties: { 'preserves-vector-representation': true, 'template-rendering-intent': 'template' },
  }),
);
