---
paths:
  - "scripts/fetch-free-roms.sh"
  - "scripts/encode-quality-*"
  - "SovEmu.Backend/SovEmu.Media/examples/**"
  - "docs/research/**"
---

# Test ROMs and measurement clips

Measurements, benches and anything else that needs a system's picture use content that anyone
can fetch and rerun. Never use a commercial game, the production library or the NAS.

## Use Artemio's 240p Test Suite first

For a system that isn't covered yet, take the first of these that exists for it:

1. **Artemio Urbina's 240p Test Suite** (GPL-2.0, [itch.io](https://artemiourbina.itch.io/240p-test-suite),
   source at `ArtemioUrbina/240pTestSuite`). It has builds for Genesis, Sega CD, 32X, PC Engine
   (HuCard and CD), SNES, Neo Geo, N64, Dreamcast, GameCube and Wii. Use the plain build, not the
   `_emu` one, unless the plain one doesn't run in the core.
2. **A port of the suite**: pinobatch's `240p-test-mini` (NES, Game Boy, GBA) or filipalac's
   `240pTestSuite-PS1`.
3. **Other openly licensed homebrew**, for gameplay the suite doesn't have (*Nova the Squirrel*,
   *Castle Platformer*, *uCity*). Only use homebrew that has a licence and a published build, and
   only where its art is the author's own: no fan games built on a company's characters.

The suite is the default because every system gets the same patterns. Its colour bars,
colour-bleed check and scroll test can be compared across systems, as #263 and #266 did.

## Fetching

- **Every ROM is a line in `scripts/fetch-free-roms.sh`**: system folder, library file name,
  SHA-256, URL, the path inside the zip, and the licence with its source.
- **The checksum pins the file**, not the URL.
  - itch.io uploads are `itch:<user>/<game>/<upload id>`. The script follows the page's
    free-download flow, because itch.io's links expire.
  - The upload id is in the page's HTML (`data-upload_id`).
  - When the author uploads a new build, update the line: checksum, file name and version.
- **A disc is its `.cue` and `.bin`, comma-separated.** Both are copied and the cue is repointed.
- **Name the library file after the suite** (`240p Test Suite.<ext>`), with an extension the
  system's entry in `systems.rs` accepts.
- **Fetched ROMs stay in the gitignored `data/roms/`.** Never commit one.

## Clips

- **Each clip is a line in `scripts/encode-quality-captures.sh`**: `<system id>-240p-<test>`,
  where the system id comes first because the bench reads it from the name.
- **For the suite, take two clips a system:**
  - `colour`: about ten seconds of colour bars, then the colour-bleed check.
  - `scroll`: the scroll test, which is the suite's closest thing to gameplay and the only clip
    that uses the whole bitrate.
- **Use a lead-in** (400 frames suits the suite) so the boot screens aren't scored.
- **The menus' buttons differ by core** (`--hold` names):

  | Core | Confirm | Back |
  |---|---|---|
  | fceumm, gambatte, mgba (pinobatch's ports) | `a` | `b` |
  | genesis_plus_gx | `y` | `start` |
  | mednafen_pce_fast | `a` | `b` |
  | pcsx_rearmed (PS1 port) | `b` | `x` |

  Some tests open a mode dialog first: press confirm once more.
- **Check a clip before benching it.** Look at frames across the whole run (every 150 is
  enough), and confirm the frame size never changes.
- **Never commit captures, crops or screenshots**: they are the games' pictures. The report
  commits only numbers, charts and each clip's `command.txt`.
