# Assets

Images the web client is built with. Vite fingerprints them into `dist/assets/`.

## `backgrounds/`

The graphics behind every screen (`src/lib/backgrounds.ts` picks one per visit). SovEmu's own,
drawn for it: transparent SVGs, 1920×1080, laid over the surface colour.

| File          | What it is                                               |
| ------------- | -------------------------------------------------------- |
| `dither.svg`  | An ordered (Bayer) dither falling away from two corners. |
| `lattice.svg` | Outlines of controller parts on a staggered grid.        |
| `screens.svg` | Nested 4:3 rounded rectangles from two corners.          |

## `systems/`

One photo per system, named by its id (`src/lib/systemPhotos.ts`): WebP cut-outs on a
transparent background, 640 px on the longer side. All are from Wikimedia Commons and are in
the public domain or CC0, so they need no attribution; a new photo must be too. Never add a
photo under a licence that asks for attribution or share-alike.

`scripts/fetch-system-photos.py` downloads the photos it lists (system id → Commons file), refuses
any that is not public domain or CC0, and writes each as a tile here. A new photo is a line in
its `PHOTOS`, then an entry in `BUILTIN_PHOTOS` (`src/appearance.rs`) and a row below.

| File                 | Commons file                                                     | Licence       | Author                                           |
| -------------------- | ---------------------------------------------------------------- | ------------- | ------------------------------------------------ |
| `nes.webp`           | `NES-Console-Set.png` (white background removed)                 | Public domain | Evan-Amos                                        |
| `snes.webp`          | `SNES US.png`                                                    | Public domain | Evan-Amos (derivative)                           |
| `gb.webp`            | `Game-Boy-FL.png`                                                | Public domain | Evan-Amos (background removed by a later editor) |
| `gbc.webp`           | `Nintendo-Game-Boy-Color-FL.png`                                 | Public domain | Evan-Amos, modified by Gunnar.offel              |
| `gba.webp`           | `Nintendo-Game-Boy-Advance-Purple-FL.png`                        | Public domain | Evan-Amos                                        |
| `megadrive.webp`     | `Sega-Genesis-Mod1-Set.png`                                      | Public domain | Evan-Amos                                        |
| `mastersystem.webp`  | `Sega-Master-System-Set.png`                                     | Public domain | Evan-Amos                                        |
| `psx.webp`           | `PSX-Console-wController.png`                                    | Public domain | Evan-Amos                                        |
| `fbneo.webp`         | `Arcade cabinet.png` (a drawing, not a photo)                    | CC0           | publicdomainq                                    |
| `neogeo.webp`        | `Neo-Geo-AES-FL.png`                                             | Public domain | Evan-Amos                                        |
| `gamegear.webp`      | `Game-Gear-Handheld.jpg` (white background removed)              | Public domain | Evan-Amos                                        |
| `n64.webp`           | `N64-Console-Set.png`                                            | Public domain | Evan-Amos                                        |
| `dreamcast.webp`     | `Sega-Dreamcast-Console-FL.png`                                  | Public domain | Evan-Amos, modified by Gunnar.offel              |
| `gamecube.webp`      | `GameCube-Console-Set.png`                                       | Public domain | Evan-Amos                                        |
| `ps2.webp`           | `PS2-Versions.png`                                               | Public domain | Evan-Amos                                        |
| `segacd.webp`        | `Sega-CD-Model2-Set.png`                                         | Public domain | Evan-Amos                                        |
| `sg1000.webp`        | `Sega-SG-1000-Console-Set.jpg` (white background removed)        | Public domain | Evan-Amos                                        |
| `sega32x.webp`       | `Sega-Genesis-Model2-32X.png`                                    | Public domain | Evan-Amos                                        |
| `pcengine.webp`      | `PC-Engine-Console-Set.png`                                      | Public domain | Evan-Amos                                        |
| `pcenginecd.webp`    | `NEC-TurboGrafx-16-CD-FL.png`                                    | Public domain | Evan-Amos                                        |
| `atari2600.webp`     | `Atari-2600-Wood-4Sw-Set.png`                                    | Public domain | Evan-Amos                                        |
| `atari7800.webp`     | `Atari-7800-Console-FL.jpg` (white background removed)           | Public domain | Evan-Amos                                        |
| `lynx.webp`          | `Atari-Lynx-Handheld-Angled.jpg` (white background removed)      | Public domain | Evan-Amos                                        |
| `ngp.webp`           | `Neo-Geo-Pocket-Anthra-Left.jpg` (white background removed)      | Public domain | Evan-Amos                                        |
| `ngpc.webp`          | `Neo-Geo-Pocket-Color-Blue-Left.jpg` (white background removed)  | Public domain | Evan-Amos                                        |
| `wswan.webp`         | `WonderSwan-Black-Left.jpg` (white background removed)           | Public domain | Evan-Amos                                        |
| `wswanc.webp`        | `WonderSwan-Color-Blue-Left.jpg` (white background removed)      | Public domain | Evan-Amos                                        |
| `virtualboy.webp`    | `Virtual-Boy-Set.png`                                            | Public domain | Evan-Amos                                        |
| `intellivision.webp` | `Mattel-Intellivision-Console-FL.jpg` (white background removed) | Public domain | Evan-Amos                                        |
| `colecovision.webp`  | `ColecoVision-wController-L.png`                                 | CC0           | OliverGalvin                                     |
| `vectrex.webp`       | `GCE-Vectrex-Bare-FL.jpg` (white background removed)             | Public domain | Evan-Amos                                        |
