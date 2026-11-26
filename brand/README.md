# SovEmu's logo

The mark is Sovereign's shield with an 8-bit S in place of Sovereign's S: SovEmu as Sovereign's
retro-games sibling. The icon is the mark in white on SovEmu's blue, `#4169E1` (the web client's
accent). The wordmark is "Sov" and "Emu" in Inter Bold, "Emu" in the blue.

| File | What |
|---|---|
| `sovemu-mark.svg` | The mark alone, white, 282×366 |
| `sovemu-icon.svg` | The icon on a rounded 512 tile, for favicons and anywhere a rounded tile is wanted |
| `sovemu-icon-square.svg` | The icon full bleed, for platforms that round the corners themselves |
| `sovemu-wordmark.svg` | The name, outlined, for dark backgrounds |
| `png/` | The icon as PNG: full-bleed squares at 1024, 512, 192, 180 and 114 (store listings, opaque), and the rounded tile at 512 and 192 |
| `sovereign-shield.svg` | Sovereign's shield outline, copied from `Sovereign.Design/brand/sovereign-logo.svg` |
| `fonts/` | Inter Bold and its licence (SIL OFL), copied from Sovereign.Design |

## Every size, in every client

`generate.mjs` draws the mark once and writes everything above, plus each client's copies:

| Client | Files |
|---|---|
| Web | `SovEmu.Web/public/favicon.svg`, `apple-touch-icon.png`, `sovemu-mark.svg` (the brand next to the name) |
| Docs | `SovEmu.Docs/public/favicon.svg`, `apple-touch-icon.png` |
| Android | the launcher icon's foreground and themed layer (`common/feature/.../drawable/ic_launcher_foreground.xml`; the background is `#4169E1` in `values/ic_launcher_background.xml`), the Android TV and Fire TV banner (`banner.xml`), and the design system's mark (`common/designsystem/.../drawable/sovemu_mark.xml`) |
| iOS | `Apps/Mobile/Assets.xcassets`: `AppIcon` (one opaque 1024 image) and `SovEmuMark` (a template image) |

Never edit an output: change `generate.mjs` or a source here, then run it.

```bash
cd brand
npm ci
node generate.mjs                    # the clients are this repository's submodules
node generate.mjs --clients <dir>    # or checkouts of them in <dir>
```

Then commit each client's changes in its own repository, with a PR there, as for any change that
spans the repositories.
