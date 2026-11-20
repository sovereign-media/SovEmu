# SovEmu.Library (`sovemu-library`)

The ROM library, without the database. It reads files and asks services, and the server
(`SovEmu.Server`) stores what it learns in its tables. Nothing here opens the database or knows
about the API.

| Module | What |
|---|---|
| `systems` | The systems table: ids, names, Recalbox folder names and aliases, extensions, cores |
| `scan` | Walking a library into games: which system a file is, parts (discs, disks, tapes, sides) folded into one game, soft patches beside a game, artwork file names |
| `labels` | Release labels for games that share a title on a system (`Game.release`) |
| `bios` | Which BIOS files each system needs, and checking a folder for them |
| `info` | Game-information clients: FBNeo DATs, LaunchBox (its dump), ScreenScraper, TheGamesDB |
| `cache` | The local copy of games from a network library (`cache` mode) |
| `mirror` | Syncing a whole network library to local disk (`mirror` mode) |
| `decorations` | Decoration (bezel) sets in Batocera's layout, and the built-in set |
| `text` | Folding text for comparison (case and accents) |

Only what the server uses is `pub`; the rest is `pub(crate)`, so it can change without touching
the server.

```bash
cargo test -p sovemu-library
# Against a real LaunchBox dump:
SOVEMU_LAUNCHBOX_DUMP=/path/to/Metadata.zip cargo test -p sovemu-library real_dump -- --ignored --nocapture
```
