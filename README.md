# SovEmu

Self-hosted console emulation streamed over WebRTC. A Rust server runs
[libretro](https://www.libretro.com/) cores and streams video, audio and input to web, Android
and iOS clients.

SovEmu ships no games and no BIOS files. Point it at a library of your own: dumps of games you
own, and BIOS files from consoles you own. `scripts/fetch-free-roms.sh` fetches a few freely
licensed homebrew games to try it with.

> **Status:** Phase 3. Sign in, pick a system and a game, play in the browser over WebRTC, save
> and continue later. See [docs/ROADMAP.md](docs/ROADMAP.md).

## Repository layout

| Path | What |
|---|---|
| `SovEmu.Backend/` | The Rust workspace: libretro bindings, the frontend, the worker, the test core |
| `brand/` | The logo: its sources and the generator that writes every client's icons ([brand/README.md](brand/README.md)) |
| `docs/` | [Architecture](docs/ARCHITECTURE.md) and [roadmap](docs/ROADMAP.md) |
| `scripts/` | `fetch-cores.sh` downloads libretro cores for local testing |
| `SovEmu.Web/` | The web client: Vite + React + TanStack Router, Tailwind, Radix UI and Lucide icons, served by the Rust server |
| `SovEmu.Android/`, `SovEmu.iOS/` | Native clients, as submodules (`git submodule update --init --remote`) |
| `SovEmu.Docs/` | The documentation site (Starlight), as a submodule |

## Container

SovEmu ships as one self-contained Debian image for x86_64, the same on every host, with or
without a GPU. The libretro cores come prebuilt from a libretro stable release (1.22.2), pinned
by version and checksum in [scripts/cores.list](scripts/cores.list), and every core is checked
for loading during the build. libretro's stable releases have no Linux arm64 cores, so the image
is x86_64 only; an arm64 machine (a Mac) builds and runs it under x86_64 emulation.

```bash
scripts/fetch-free-roms.sh            # free homebrew into data/roms, or add your own
docker compose up --build             # then open http://localhost:8080
```

The first visit creates the admin account. `./data` is mounted at `/sov-data` (the database,
`data/bios/` for BIOS files, see [BIOS files](#bios-files), `data/saves/` and `data/artwork/`),
and `./data/roms` read-only at `/sov-library`, the library: one folder per system (`nes/`,
`snes/`, `gb/`, `gbc/`, `gba/`, `megadrive/`, `mastersystem/`, `gamegear/`, `segacd/`,
`pcengine/`, `atari2600/`, `psx/`, `n64/`, `dreamcast/`, `gamecube/`, `ps2/`, …; the full list is
under [Supported systems](#supported-systems)). Browsers on other machines need
`SovEmu__Streaming__AdvertiseAddress` set to this host's LAN address (it defaults to `127.0.0.1`), plus UDP
50000-50009 reachable.

### Folders

What the image ships and what SovEmu writes are kept apart. `/opt/sovemu` is the runtime,
read-only and replaced by every upgrade; `/sov-data` is one volume holding everything SovEmu
writes, kept across upgrades; the library is its own read-only mount. The two `artwork/` folders
have the same layout: a file in `/sov-data/artwork` replaces the built-in one at the same path
(copy a path from `/opt/sovemu/artwork`, put your own file there).

```text
/opt/sovemu/  (image, read-only)                /sov-data/  (volume, writable)
├── artwork/                                    ├── artwork/
│   ├── backgrounds/  dither, lattice, screens  │   ├── backgrounds/        your own, or replacements
│   ├── systems/      <system>.webp photos      │   ├── systems/            <system>.webp replaces a photo
│   ├── decorations/  sovemu/ frames            │   ├── decorations/        your sets (Batocera layout)
│   └── ratings/esrb/ ec|e|e10|t|m|ao|rp.png    │   ├── ratings/esrb/       <rating>.png replaces an icon
│                                               │   └── games/<game id>/    box, shot, marquee .webp; video.mp4
├── bin/          sovemu-server, sovemu-worker  ├── sovemu.db, server.json
├── cores/        cores, manifest.json, system/ ├── saves/<user>/<system>/
└── web/          the web client                ├── bios/                   any layout
                                                ├── metadata/fbneo/
                                                ├── backups/, library/ (mirror mode)
                                                └── cache/                  safe to delete
/sov-library/  (read-only)  the ROMs, never written
```

An image that finds no database in `/sov-data` but one in `/data` (where data lived before)
refuses to start, so a volume still mounted at the old place never looks like an empty server.

## BIOS files

BIOS files live on the server, in `data/bios/` (`SovEmu__Paths__Bios`), in **any layout**: SovEmu
indexes the whole folder and finds each file a core needs by its MD5 (a known dump, under any
name) or else by its name in any letter case. A Batocera `bios/` folder, or a read-only share,
works as it is. The recommended layout is a folder per system, named like its folder of games, as
below; the server creates those folders when it can write there. **Systems → BIOS files** in the
web client checks each file against libretro's list of known dumps (`System.dat`) and says where
it was found, and the Systems menu marks a system that cannot start games without one. It is one table, a row per file, for the systems
that have games: sort it by any column, or search it by system or file name.

| Recommended folder | Files | Without them |
|---|---|---|
| `bios/ps2/` | any PS2 BIOS dump, any name (`*.bin`) | **no games start** |
| `bios/neogeo/` | `neogeo.zip` (or beside the games in `roms/neogeo/`) | **no games start** |
| `bios/psx/` | one of `scph5501.bin`, `scph5500.bin`, `scph5502.bin`, `scph1001.bin`, `scph7001.bin`, `scph101.bin` | built-in BIOS: most games play |
| `bios/dreamcast/` | `dc_boot.bin`; optionally `dc_flash.bin` | built-in BIOS: many games play |
| `bios/gba/` | `gba_bios.bin` | built-in BIOS: almost every game plays |
| `bios/fbneo/` | `neogeo.zip`, for Neo Geo games in the arcade folder | those games only |
| `bios/nes/` | `disksys.rom` | no Famicom Disk System games |
| `bios/gb/`, `bios/gbc/` | `gb_bios.bin`, `gbc_bios.bin` | no boot logo |
| `bios/megadrive/`, `bios/mastersystem/`, `bios/gamegear/` | `bios_MD.bin`; `bios_E.sms`, `bios_U.sms`, `bios_J.sms`; `bios.gg` | no start-up screen |
| `bios/segacd/` | `bios_CD_U.bin`, `bios_CD_E.bin`, `bios_CD_J.bin`, one per region you play | **no games of that region start** |
| `bios/pcenginecd/` | `syscard3.pce` | **no CD games start** (HuCards in `pcengine/` need nothing) |
| `bios/lynx/` | `lynxboot.img` | **no games start** |
| `bios/intellivision/` | `exec.bin` and `grom.bin` | **no games start** |
| `bios/colecovision/` | `colecovision.rom` (or `coleco.rom`) | **no games start** |
| `bios/atari7800/` | `7800 BIOS (U).rom` or `7800 BIOS (E).rom` | no start-up screen |
| `bios/gamecube/` | `USA/IPL.bin`, `EUR/IPL.bin`, `JAP/IPL.bin` | no start-up animation |

Each core finds its files where it expects them: every session gets a folder of links, each file
under the name and in the place its core reads it (Flycast's `dc/`, PCSX2's `pcsx2/bios/`,
Dolphin's `dolphin-emu/Sys/GC/USA/IPL.bin`), wherever it is in the BIOS folder. Nothing in the
BIOS folder is renamed, moved or written: files a core creates beside its BIOS (Flycast's
`dc_nvmem.bin`, PCSX2's NVRAM) last the session.

## 3D systems and the GPU

N64, Dreamcast, GameCube and PS2 render with OpenGL. At startup the server tries every GPU the
container can see: it makes a context on each, draws a frame and reads it back. It uses the
first one that works. Without one, it falls back to Mesa's software renderer (llvmpipe), which is
fine for many N64 games and slow for GameCube and PS2. The startup log says which:

```
GPU probe:
  /dev/dri/renderD128 (amdgpu, hardware): works: AMD Radeon RX 6600 (radeonsi, navi23, …)
  software renderer (software): works: llvmpipe (LLVM 22.1.3, 128 bits)
3D systems render on the GPU: AMD Radeon RX 6600 (radeonsi, navi23, …)
```

The API's `gpu` query (admin) returns the same, and each 3D system's `rendering` is `GPU`,
`SOFTWARE` or `OFF`.

- **Docker, AMD or Intel:** `docker compose -f compose.yaml -f compose.gpu.yaml up -d` passes
  `/dev/dri` through. Set `SOVEMU_RENDER_GID` in `.env` to the group that owns the render node
  (`stat -c %g /dev/dri/renderD128` on the host).
- **Docker, NVIDIA:** install NVIDIA's [container
  toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/) on the host, then
  `docker compose -f compose.yaml -f compose.nvidia.yaml up -d`. The toolkit adds the host's
  NVIDIA driver to the container; the image asks it for OpenGL/EGL, Vulkan and NVENC.
- **Kubernetes:** set `gpu.resources` to your GPU device plugin's resource (`amd.com/gpu: 1`,
  `gpu.intel.com/i915: 1`, `nvidia.com/gpu: 1`), or use `gpu.hostDevice`. For NVIDIA, also set
  `gpu.runtimeClassName: nvidia` if the cluster does not make NVIDIA's runtime the default.
- **Which GPUs:** AMD (radeonsi) and Intel (iris, crocus) through Mesa in the image; NVIDIA
  through NVIDIA's own driver (or Mesa's nouveau/NVK on the open-source driver).
- **Dolphin** needs its `Sys` data folder. The image ships it beside the cores and overlays it on
  the BIOS files, so neither is changed.
- **PS2 needs a BIOS dump**, and Dreamcast plays best with one: see [BIOS files](#bios-files).
- **Memory cards** for Dreamcast (VMUs) and PS2 are kept per game in the player's save folder,
  never under the shared BIOS folder.
- **GameCube needs `/dev/shm`**, at least 1 GB: Dolphin maps its emulated memory there.
  `compose.yaml` sets `shm_size` and the chart mounts one (`shmSize`). Docker's default 64 MB
  makes Dolphin die with a bus error.
- **Analog sticks:** for 3D systems, the web client sends the gamepad's sticks and triggers as
  analog values. For 2D systems the left stick still acts as a D-pad.

## Emulator settings

Each emulator has its own settings: rendering, internal resolution, frame skipping and so on
(43 for Snes9x, 113 for Dolphin). Admins change them per system on the **Settings** page: pick
the system from the dropdown, and its settings are grouped (System, Video, Audio, Input, then the
emulator's own groups), each with the emulator's explanation of what it does. A few emulators
list their settings only once a game is loaded: the worker loads a tiny stand-in game for those
(an empty NES cartridge, a GameCube program that does nothing) to read them. Arcade has no such
stand-in, so its page fills in after any arcade game has been played once. The worker speaks
libretro's core options v2, so groups, explanations and value labels come from the emulator
itself; settings an emulator leaves ungrouped are placed by keyword. Changes apply to the next
game started.

A game starts with each setting taken from these, in order, the last one winning:

1. the emulator's default;
2. SovEmu's default for the system (Dreamcast memory cards per game, for example);
3. the admin's choice;
4. the environment (`SovEmu__Systems__<id>__Options__<name>`).

The environment always wins, and those settings show as locked on the page. The page also
lists the admin's changes as environment variables to copy into `.env` or the Helm values, so
the configuration never depends on the page. In the API: the `emulatorSettings` query and the
`changeEmulatorSettings` mutation (admin).

**Run-ahead**, on the same page, hides a game's own input lag. Most games act on a button a
frame or more after reading it; with run-ahead the worker runs each frame, saves its state,
runs N more frames with the same input, shows the last one's picture and puts the state back.
A press then shows up N frames (about 17 ms each at 60 fps) sooner, while the game's real
timeline and its sound are untouched. Most games need 1 or 2; too many and the game visibly
skips ahead. Each frame costs N more emulated frames and a save state, so it is only for the
systems that render in software (not N64, Dreamcast, GameCube or PS2), and a worker that
cannot keep up runs fewer frames ahead by itself. It is on by default, 1 frame on every
system that supports it; change it per system with `SovEmu__Systems__<id>__RunAhead` (0 to 4,
0 turns it off; this wins) or as an admin (`setRunAhead`).

## Loading progress

Starting a game can take a while on a slow library, so the player shows a loading bar. It
covers each step: saving the previous game, starting the emulator, opening the core and reading
the game, with the megabytes read and the seconds taken. The bar is exact when SovEmu reads the
file. When a core reads a disc itself, the bar eases forward while the megabytes count up.

Clients give `startSession` a `startId` and poll the `startProgress(startId)` query while the
mutation waits. Clients that don't send one work as before.

## The API

SovEmu's API is GraphQL, at `POST /graphql`; open `/graphql` in a browser for GraphiQL. The web,
Android and iOS clients all use it. The schema, with every type and field documented, is
[SovEmu.Backend/SovEmu.Server/schema.graphql](SovEmu.Backend/SovEmu.Server/schema.graphql).

- **Sign-in** is OAuth 2.1 with OpenID Connect over it (`SovEmu.Identity`, on the
  [`oauth-as`](https://crates.io/crates/oauth-as) crate). Discovery is at
  `/.well-known/openid-configuration`, and every API request carries `Authorization: Bearer
  <access token>`. Three clients are built in, all public, as in Sovereign. Their ids are made
  once per install and kept in `data/server.json` (`webClientId`, `deviceClientId`,
  `mobileClientId`). Replacing an id there removes the old registration, and every sign-in under it, on
  the next start. Apps learn the ids from `bootstrapConfig`; there is no setting for them.
  - The **web** client: the authorization code flow with PKCE (S256, required) and refresh
    tokens. It has no password grant; the password goes only to the server's own sign-in page
    (`/login/`, which posts to `/oauth/signin`).
  - The **device** client, for TVs and any app signing in with a code or a password:
    - the device grant (`/oauth/device_authorization`): the TV shows a code and the address
      `/link`, the web client's page, where someone signed in approves it (`/device`, the old
      address, redirects there);
    - or the password fallback, `POST /oauth/password` (`client_id`, `username`, `password`,
      `scope`), which answers like the token endpoint. `SovEmu__Authentication__PasswordSignIn=false` turns it
      off.
    A phone or tablet already signed in can approve a TV's code too (*Sign in a TV*): the
    `approveDeviceCode(userCode)` mutation, as the signed-in user.
  - The **mobile** client, for the phone and tablet apps signing in through the system browser:
    the authorization code flow with PKCE back to the apps' own scheme
    (`media.sovereign.sovemu:/oauth2redirect`). No password and no device grant.

  Token lifetimes are the server's, the same for every client
  (`SovEmu__Authentication__AccessTokenMinutes` and the session days).

  An app's first question is the anonymous `bootstrapConfig` query, as Sovereign's apps ask
  theirs. It names the issuer, the client ids and every endpoint URL for the address the app
  reached (or says sign-in isn't available there), and leaves out the password endpoint when the
  fallback is off. Apps hard-code nothing but `/graphql`.

  Refresh tokens rotate (`POST /oauth/token`, `grant_type=refresh_token`), and a used one
  presented again ends that sign-in. `/oauth/revoke` signs an app out. Access tokens are looked
  up on every request, so a revoked sign-in or a changed role applies at once. First-run `setup`
  is still a mutation; there is no sign-in mutation.
- **Roles**: `VIEWER` plays; `ADMIN` also runs the server. `User.role` (and `isAdmin`).
- **Errors** come back in `errors`, each with an `extensions.code`: `UNAUTHENTICATED`,
  `FORBIDDEN`, `NOT_FOUND`, `BAD_REQUEST`, `CONFLICT`, `GONE`, `TOO_MANY_REQUESTS`, `UNAVAILABLE`.
- **Files** (artwork, videos, save screenshots) are plain HTTP under `/media/`, at the URLs the
  API returns (`Game.mediaUrl`, `SaveSlot.screenshotUrl`).
- **Playing** a game: `startSession`, then `sessionOffer` with the browser's WebRTC offer (it
  returns the answer). Media and controller input then flow over WebRTC.

## A library on a NAS

`compose.nas.yaml` mounts an SMB share **read-only**. The share should hold a Batocera-style
folder (`SOVEMU_NAS_PATH`) with `roms/<system>/` and `bios/`: only those two are mounted, `roms/`
at `/sov-library` (the system folders at its root) and `bios/` at `/sov-bios`, used as it is (any
layout), so nothing else of the share is visible in the container. Only the ROMs are read:
artwork folders beside them (`images/`, `videos/`) are skipped, and every game's artwork comes
from ScreenScraper. Put the share, credentials and mode in `.env`, which git ignores (see
`.env.example`), then:

```bash
docker compose -f compose.yaml -f compose.nas.yaml up -d
```

Choose how the server uses the share with `SovEmu__Library__Mode`:

| Mode | When | What happens |
|---|---|---|
| `direct` (default) | The NAS is on the same network | Games are read straight from the share |
| `cache` | The NAS is remote (VPN, a Cloudflare tunnel) | Played games are kept in `data/cache/games` (`SovEmu__Library__Cache__MaxGigabytes`, least recently used out). Small games are copied on first play; disc images are copied after their first session. A 32 MB GBA game: 7.9 s cold, 0.3 s warm over a tunnel. |
| `mirror` | You want a full local copy | A background sync copies the library's games into `data/library` (not its artwork folders), copying only what changed and removing what was deleted. Games are played from the copy. `SovEmu__Library__Mirror__Systems=nes,snes` limits it. Start a sync from the Library page. |

Scans run in the background, in parallel.

### Soft patches

Translations, fixes and hacks come as IPS, UPS or BPS patches. SovEmu applies them as the game
loads and never writes the library. Put a patch where the scanner looks for it:

```
roms/snes/Mother 2 (Japan).sfc
roms/snes/Mother 2 (Japan).bps                 # beside it, same name: applied by default
roms/snes/Mother 2 (Japan)/English v1.1.bps    # in a folder named as the game: to choose
roms/snes/Mother 2 (Japan)/Hard Mode.ips
```

The game panel shows a **Patch** choice when a game has any: the original, or one of its patches,
per player. A patched game keeps its own saves (`Mother 2 (Japan) [English v1.1].srm`), as it is in
effect another game. UPS and BPS patches carry checksums, so one made for another dump of the
game is refused with a message rather than applied wrongly; IPS has none. Disc images are not
patched.

### Decorations (bezels)

Games are framed with artwork that fills the bars a 4:3 picture leaves on a 16:9 screen. The
server picks the frame and the clients draw it around the stream. Sets use
[Batocera's layout](https://wiki.batocera.org/en:customize_decorations_bezels), so its decoration
packs work as they are:

```
decorations/<set>/systems/snes.png    + snes.info     a system's games
decorations/<set>/games/snes/<rom>.png + <rom>.info   one game (its file name, no extension)
decorations/<set>/default.png         + default.info  everything else
```

Each `.info` gives the image's size and the margins of its transparent window
(`{"width":1920,"height":1080,"top":0,"left":240,"bottom":0,"right":240}`); without one, the window
is the largest centred 4:3 area. Sets are read from `SovEmu__Decorations__Directories`, then
`data/artwork/decorations`, then the built-in folder (`/opt/sovemu/artwork/decorations`). A set found in several of them is merged, with
the earlier folder winning for the same file. Folders are listed again with every library scan.

SovEmu's own set, `sovemu`, is a frame per system in its colour, shaped to its screen (4:3, or
10:9 and 3:2 for handhelds), drawn by the server's own code. SovEmu ships no third-party
decorations and has no downloader: packs such as the Bezel Project carry no license, so add them
yourself.

The ESRB's rating icons that a game's panel shows are built-in artwork too:
`artwork/ratings/esrb/<rating>.png` (`ec`, `e`, `e10`, `t`, `m`, `ao`, `rp`; the 2013 set from
Wikimedia Commons). Put your own PNG at that path in `data/artwork/` to replace one.
Choose the set with `SovEmu__Decorations__Set`, per system with `SovEmu__Systems__<id>__Decoration`,
and `none` turns frames off.

## Game information

The **Library** page (admins) fetches titles, descriptions, release dates, developers,
publishers, genres, player counts, ratings, ESRB age ratings, play modes, series, themes, box
art and gameplay videos. The information is stored as each game is looked up. A game's panel
shows its ESRB rating as the ESRB's own icon (served from the built-in decoration set, where an
admin can replace it; see Decorations) and its rating as stars, and Home has a **Top rated** row of the library's best rated
games (ScreenScraper's players' ratings). Its artwork follows in the background,
`SovEmu__Artwork__MaxConcurrentDownloads` at a time: box art, a screenshot (shown behind the game; in play,
else the title screen), its logo (the wheel, for the marquee) and a video. Pictures are asked for
at the size the largest screen shows them (`SovEmu__Artwork__MaxPixels`, 1000 pixels), not as a full
scan; videos are ScreenScraper's normalised ones where it has them (smaller, one encoding), and
`SovEmu__Metadata__Videos=false` skips them. Pictures are kept as lossy WebP
(`SovEmu__Artwork__WebpQuality`), and every client asks for artwork at the width it shows it: the
server makes a WebP copy at that width the first time and
keeps it in `data/cache/artwork`, so a grid of covers moves a few kilobytes a card rather
than the original files. Clearing the cache clears the copies; they are made again as needed. A system's game cards take the shape of its boxes
(SNES and N64 landscape, CD systems square, cartridge systems portrait), so box art fills them;
a box of another shape (a Super Famicom's) is fitted inside its card. Sources, asked in the order `SovEmu__Metadata__Sources`
gives (`fbneo,screenscraper`):

1. **FinalBurn Neo's game lists**, for arcade and Neo Geo sets. No account needed:
   `mslug` becomes *Metal Slug - Super Vehicle-001* (1996, Nazca).
2. **[ScreenScraper](https://www.screenscraper.fr)**, the database Recalbox, Batocera, RetroBat and
   ES-DE scrape from. It matches each file by its MD5, size and name, so it also finds hacks,
   translations and homebrew. Every request carries two sets of credentials: the *developer*
   ones say which program asks, and SovEmu's own are built into the released image, so there is
   nothing to request; the *user* ones say who asks. Without a user account lookups run as a
   guest, the slowest level and the first turned away when ScreenScraper is busy; a free
   account (`SovEmu__Metadata__ScreenScraper__Username`, `SovEmu__Metadata__ScreenScraper__Password`) is steadier, and
   contributing or donating raises its limits. A build of your own has no developer credentials
   unless you set `SovEmu__Metadata__ScreenScraper__DevId` and `SovEmu__Metadata__ScreenScraper__DevPassword` (they also
   replace the built-in ones); without any, ScreenScraper is not asked, and the Library page says
   so.

ScreenScraper's limits depend on the account: its level, and what it has contributed (data or
money) raise the requests it may have open at once, per minute and per day. SovEmu reads the
account at startup and before each lookup (`ssuserInfos.php`, which spends no request), looks up
as many games at once as the account allows (`SovEmu__Metadata__ScreenScraper__Threads` caps it), spaces
requests by its per-minute limit, and downloads box art no faster. The Library page shows the
account's level and limits. ScreenScraper reports problems in French; SovEmu shows them in English.

Each game is asked about until every field a game page shows is filled; an earlier source's
field wins. ScreenScraper's own id for each game is kept, so looking a game up again asks for
that game instead of hashing its file. The Library page shows a lookup as it runs: the game and
the source being asked, each system's progress, an estimate of the time left, what each source
found, ScreenScraper's quota for today (requests, and lookups that found nothing, which it caps
on its own), and the last games looked up.

A lookup stops using a service that rejects the credentials, runs out of quota or is closed, and
the next one carries on with the games it did not reach. Games no service found are asked again
once a service they were not asked is configured, so adding credentials later fills in the
library on the next scan. A lookup can also refresh only some fields, ask only some sources, or
take the games looked up more than so many days ago (`fetchMetadata`); with
`SovEmu__Metadata__RefreshDays` the lookup after each scan does that on its own. A refresh replaces
only the fields it was asked for, and never clears one a source no longer knows.

After every scan, before its lookup, a **library check** (the `reconcile` job, on the Jobs page)
keeps what was downloaded in step with the library: a game whose ROM is gone loses its
information, its downloaded artwork and video, and its resized copies; a downloaded file that has
gone missing from `data/artwork/games/` is downloaded again. An admin can run it from the Jobs
page, and then it also looks up games without information even when `SovEmu__Metadata__AutoLookup` is off.

## Supported systems

A system is shown once its folder has at least one game: one with none is left out of every list
(browsing, search, the BIOS check and the admin's per-system pages), however many SovEmu can play.
Folder names follow Recalbox and Batocera; the aliases each system also accepts are in
`SovEmu.Library/src/systems.rs`.

| Folder | System | Core |
|---|---|---|
| `nes` | NES / Famicom | FCEUmm |
| `snes` | Super Nintendo | Snes9x |
| `gb`, `gbc` | Game Boy, Game Boy Color | Gambatte (mGBA) |
| `gba` | Game Boy Advance | mGBA |
| `virtualboy` | Virtual Boy | Beetle VB |
| `megadrive`, `mastersystem`, `gamegear`, `sg1000` | Mega Drive / Genesis, Master System, Game Gear, SG-1000 | Genesis Plus GX |
| `segacd` | Mega-CD / Sega CD | Genesis Plus GX |
| `sega32x` | 32X | PicoDrive |
| `pcengine`, `pcenginecd` | PC Engine / TurboGrafx-16, and its CD games | Beetle PCE Fast |
| `atari2600` | Atari 2600 | Stella |
| `atari7800` | Atari 7800 | ProSystem |
| `lynx` | Atari Lynx | Handy |
| `ngp`, `ngpc` | Neo Geo Pocket, Neo Geo Pocket Color | Beetle NeoPop |
| `wswan`, `wswanc` | WonderSwan, WonderSwan Color | Beetle WonderSwan |
| `intellivision` | Intellivision | FreeIntv |
| `colecovision` | ColecoVision | Gearcoleco |
| `vectrex` | Vectrex | vecx |
| `psx` | PlayStation | PCSX-ReARMed |
| `fbneo`, `neogeo` | Arcade, Neo Geo | FinalBurn Neo |
| `n64`, `dreamcast`, `gamecube`, `ps2` | Nintendo 64, Dreamcast, GameCube, PlayStation 2 | Mupen64Plus-Next, Flycast, Dolphin, LRPS2 (3D: see [3D systems and the GPU](#3d-systems-and-the-gpu)) |

Everything above the 3D systems renders in software and needs little CPU. Intellivision and
ColecoVision games that lean on the keypad are awkward on a gamepad: the keys SovEmu maps are on
each system's controls diagram.

## Configuration

Every setting is an environment variable. There's nothing to set up by clicking through the app.
`compose.yaml` loads `.env` whole, so put settings there; `.env.example` lists them all. The same
names work as `--flags` on `sovemu-server`. An empty value means "unset". Secrets come the same
way: put Docker or Kubernetes secrets in the environment. The server logs its effective
configuration at startup, with secrets redacted.

Names follow the hierarchy .NET (and Sovereign) use: `SovEmu__<Area>__<Name>`, PascalCase levels
joined by a double underscore, read in any letter case (`SOVEMU__HTTP__LISTEN` works too). A list
takes one entry per index, `SovEmu__Network__TrustedProxyNetwork__0`, `__1`, …, in order; one plain
value (`SovEmu__Http__PublicUrl=https://…`) still works for a list of one.

| Variable | Default | What |
|---|---|---|
| **Accounts** | | |
| `SovEmu__Authentication__AdminUsername`, `SovEmu__Authentication__AdminPassword` | unset | The admin. Created at startup, and the password follows this value (changing it signs old sessions out). Without it, the first visit asks for an admin. |
| `SovEmu__Users__<n>__Username`, `__Password`, `__Role` | none | More accounts, kept in step like the admin (see below). `__Role` is `viewer` (default) or `admin` |
| `SovEmu__Users__Prune` | off | Delete accounts not declared above at startup |
| `SovEmu__Authentication__SessionDays` | `90` | How long a sign-in lasts before signing in again (an app's counts from signing in; refreshing does not extend it), and the browser's sign-in at the server. 0: until signed out |
| `SovEmu__Authentication__Tokens__AccessTokenMinutes` | `60` | How long an access token lasts before the app refreshes it |
| `SovEmu__Authentication__Tokens__RefreshRetrySeconds` | `30` | How long a just-used refresh token is honoured again (two tabs refreshing at once); after that, presenting it ends the sign-in. 0: strictly once |
| `SovEmu__Http__PublicUrl__<n>` | `localhost`, `127.0.0.1` and `SovEmu__Streaming__AdvertiseAddress` on the HTTP port | The addresses people reach the server at, one per index (`__0=https://games.example.com`, `__1=http://192.168.1.10:8080`). Each issues sign-in tokens; signing in elsewhere is refused. Set it when you use a hostname or a proxy |
| `SovEmu__Authentication__PasswordSignIn` | `true` | Whether the apps may sign in with a username and password, as well as with a code shown on the screen |
| `SovEmu__ClientErrors__Enabled` | `true` | Whether what goes wrong on a client (a failed connection, a stalled picture, a screen error) is logged as `client event` and kept for the admin Client errors page (`SovEmu__ClientErrors__RetentionDays`). Off, clients' reports are dropped |
| `SovEmu__Authentication__TokenSigningKey` | generated, in `data/server.json` | The secret the ID-token signing key (ES256) is derived from. A secret |
| `SovEmu__Authentication__Lockout__MaxFailures` | `5` | Failed sign-ins before a username is locked out (0: never) |
| `SovEmu__Authentication__Lockout__MaxFailuresPerAddress` | `20` | Failed sign-ins from one address, across usernames, before it is locked out (0: never) |
| `SovEmu__Authentication__Lockout__Minutes` | `5` | How long the lockout lasts |
| **Library** | | |
| `SovEmu__Paths__Data` | `data` (`/sov-data`) | Everything SovEmu writes: database, saves, artwork, cache, mirror (see [Folders](#folders)) |
| `SovEmu__Paths__Library` | `<data>/roms` (`/sov-library`) | The library, one folder per system, read-only |
| `SovEmu__Paths__Bios` | `<data>/bios` | BIOS files, in any layout; a read-only share is fine (see [BIOS files](#bios-files)) |
| `SovEmu__Library__Mode` | `direct` | `direct`, `cache` or `mirror` (see above) |
| `SovEmu__Library__Cache__MaxGigabytes` | `20` | `cache` mode: GiB kept locally |
| `SovEmu__Library__Mirror__Systems__<n>` | all | `mirror` mode: these systems only, by id, one per index |
| `SovEmu__Library__ScanIntervalMinutes` | `60` | Rescan period (0: startup only) |
| `SovEmu__Library__Mirror__IntervalMinutes` | `360` | `mirror` mode: sync period (0: startup only) |
| `SovEmu__Paths__Cores` | `cores` (`/opt/sovemu/cores`) | libretro cores |
| `SovEmu__Library__Cache__LargeGameMegabytes` | `64` | `cache` mode: larger games are cached after their session, not before |
| `SovEmu__Library__CopyThreads` | `4` | Files copied at once by the mirror sync |
| `SovEmu__Systems__<id>__Core`, `__Enabled`, `__Decoration`, `__RunAhead`, `__Prescale`, `__Limits__<limit>`, `__Options__<name>` | none | Per-system core, hiding a system, decoration set, run-ahead frames (0-4, 2D systems), pre-scale (1 or 2, see `SovEmu__Streaming__Video__Prescale`), what its games may use (`MemoryMegabytes`, `Cpus`, `Threads`, `OpenFiles`, see `SovEmu__Sessions__Limits__*`), core options (see below) |
| `SovEmu__Collections__<n>__Name`, `__Games__<m>` | none | A shared collection everyone sees, its games as `<system id>/<file name>` (`nes/Super Stand-in (USA).nes`), in order. Kept in step after each scan; locked on the admin page, which offers the collections made there in this form. A collection no longer declared is deleted |
| `SovEmu__Controllers__<n>__Id`, `__Bindings`, `__System`, `__Name` | none | Default button mapping for a controller model (`xbox`, `dualsense`, `pad:<vendor>:<product>`, `keyboard` …), as `south:B,east:A,…`: see [docs/CONTROLLERS.md](docs/CONTROLLERS.md). Wins over the admin page; a user's own mapping wins over both |
| **Game information** | | |
| `SovEmu__Metadata__AutoLookup` | `true` | Look up new games after every scan |
| `SovEmu__Metadata__Sources__<n>` | `fbneo`, `screenscraper` | Game-information sources, in the order they are asked |
| `SovEmu__Metadata__RefreshDays` | `0` | Look games up again once their information is this many days old (`0`: never) |
| `SovEmu__Metadata__ScreenScraper__DevId`, `SovEmu__Metadata__ScreenScraper__DevPassword` | SovEmu's, in released images | ScreenScraper developer credentials (which program asks); set only to use your own |
| `SovEmu__Metadata__ScreenScraper__Username`, `SovEmu__Metadata__ScreenScraper__Password` | unset | ScreenScraper account (raises the quota) |
| `SovEmu__Metadata__Regions__<n>` | `us`, `wor`, `eu`, `ss`, `uk`, `jp` | Region preference for titles, dates and box art, most preferred first |
| `SovEmu__Metadata__Languages__<n>` | `en`, `wor` | Language preference for descriptions, most preferred first |
| `SovEmu__Metadata__ScreenScraper__DelayMilliseconds` | `1200` | Pause between ScreenScraper requests, when the account does not give its per-minute limit |
| `SovEmu__Metadata__ScreenScraper__Threads` | `0` | At most this many ScreenScraper requests at once (`0`: as many as the account allows) |
| `SovEmu__Artwork__MaxConcurrentDownloads` | `4` | Box art and videos downloaded at once, in the background after a lookup (never more than the account allows) |
| `SovEmu__Artwork__MaxPixels` | `1000` | Box art is asked for, and kept, at most this many pixels wide and high (`0`: full size) |
| `SovEmu__Artwork__WebpQuality` | `80` | Quality (1-100) of the lossy WebP box art is kept in and artwork copies are made in |
| `SovEmu__Metadata__Videos` | `true` | Download gameplay videos for games with none of their own |
| `SovEmu__Metadata__ScreenScraper__Url` | ScreenScraper's | API base URL (mirrors, tests) |
| **Streaming** | | |
| `SovEmu__Http__Listen` | `0.0.0.0:8080` | Web client and API address |
| `SovEmu__Streaming__Udp__Ports` | `50000-50009` | One UDP port per concurrent session; publish the range |
| `SovEmu__Streaming__Udp__Bind` | `0.0.0.0` | Address the session ports bind to |
| `SovEmu__Streaming__Udp__Dscp` | `34` | The DSCP value every game's packets are marked with (0 to 63), asking the network to send them ahead of ordinary traffic. A Wi-Fi access point maps it to one of its queues: `34` (AF41, interactive video, as WebRTC marks a picture) goes to the video queue; `46` to the voice queue; `0` leaves packets unmarked. Routers and switches on the way may honour it, ignore it or clear it |
| `SovEmu__Streaming__AdvertiseAddress` | detected (`127.0.0.1` in compose) | The address browsers send media to |
| `SovEmu__Streaming__AdvertiseAddressV6` | `auto` | Games are offered over IPv6 too, on the same ports, so a player with IPv6 connects straight to them with nothing forwarded. `auto` offers this host's global IPv6 address when the session ports bind every address (`SovEmu__Streaming__Udp__Bind` is `0.0.0.0`) and there is one; with none (a container on a bridge network, usually) games are offered over IPv4 alone. `off` turns IPv6 off; an IPv6 address offers that one (one the host does not have is still offered, for a router or Docker that forwards it here) |
| `SovEmu__Network__PortMapping` | `true` | Ask the home router, over UPnP or NAT-PMP, to forward the session ports and the relay's port 3478 (UDP and TCP) to this server, so players away from home reach it with nothing set up on the router. Mappings are renewed while the server runs and removed when it stops. `false` asks nothing: for a network where the router must not be changed, or ports forwarded by hand |
| `SovEmu__Streaming__Video__Kbps` | `5000` | Video bitrate a stream starts at. It then adapts throughout the stream: up while the connection has headroom and down on congestion (the send-bandwidth estimate, when the stream is sending more than it leaves: an estimate the stream is already under cuts nothing, since a quiet picture makes the estimator read low on a link with room to spare. A fall is checked against the client's own report, sent once a second. Trouble it has just shown (packets lost, a keyframe asked for, or for two samples running frames arriving short or a long round trip) is believed at once. On a link that has shown no trouble yet, a client reporting a clean link overrules the estimator: it starts again from where it was and the bitrate stays, since over Wi-Fi the estimator also falls when packets merely arrive unevenly. A client that sends no stats, or a link that has shown trouble before, gets one such pass in five seconds, and a second fall is believed. A fall that is believed leaves a ceiling at 85% of what was being sent, which the bitrate does not climb past; the ceiling is tried 5% higher after 30 seconds without a fall (a try that stands doubles the next step, to 20%; one that fails goes back to the ceiling that stood and waits twice as long, to 5 minutes). One fall cuts to no less than half the ceiling), and down while the client's decoder falls behind (the stats every client sends once a second: frames decoded, frames dropped, decode time), back up as it recovers. The player is told of a change of a quarter or more, at most every 15 seconds, by a notice at the top of the screen, worded and drawn as Sovereign's ("Slow connection. Playing at a lower quality."). `video_kbps_target`, `estimate_kbps` and `client_limit_kbps` in the `session health` log line show it |
| `SovEmu__Streaming__Video__MinKbps` | `300` | The lowest video bitrate a stream adapts down to |
| `SovEmu__Streaming__Video__MaxKbps` | `12000` | The highest video bitrate a stream adapts up to (never below `SovEmu__Streaming__Video__Kbps`). Set it to `SovEmu__Streaming__Video__Kbps` for a stream that only adapts down |
| `SovEmu__Streaming__Video__Codec` | `auto` | `auto`, `vp8` or `h264`. `auto` chooses for each client as it connects: every codec this server encodes and the client decodes is priced in milliseconds a frame, and the cheapest is sent. Encoding is priced by the startup encoder test (the admin's Streaming page shows it); decoding by what that device's decoder measured the last time it played with the codec, and until it has, by what the device declares (a hardware decoder with a low-latency mode is taken as fast). Close prices go to the codec encoded on the GPU. A client that sends no capabilities gets VP8. Naming a codec sends it to every client. The `video codec chosen` log line, and each session on the admin's Sessions page, show the prices |
| `SovEmu__Streaming__Video__Encoder` | `auto` | H.264 encoder: `auto` (the GPU's: VA-API on Intel/AMD, NVENC on NVIDIA; else x264), `vaapi`, `nvenc` or `software`. One that does not open, or holds frames back, falls back to x264 |
| `SovEmu__Streaming__Video__Prescale` | per system | `1` or `2`: scale each game's picture up this many times by repeating pixels before it is encoded. At `2` every console pixel is a 2×2 block, so the codecs' 4:2:0 chroma keeps each pixel's own colour, for four times the pixels to encode and to decode. Unset, it is `2` for NES, SNES, Game Boy Color, GBA, Genesis and PC Engine, where it was measured to help (sovereign-media/SovEmu#263, #266), and `1` elsewhere (PlayStation games, mostly 3D, weren't measured); set, it is every system's, and `SovEmu__Systems__<id>__Prescale` still wins. Frames that would come out larger than 704×480 (SNES hi-res, PlayStation's wider modes) are sent at their own size |
| `SovEmu__Streaming__Video__ScreenContent` | `0` | VP8's screen-content mode, tuned for flat colour and sharp edges: `0` off, `1` on, `2` on with more aggressive rate control, which may drop frames. H.264 has no such mode |
| `SovEmu__Streaming__Audio__Kbps` | `128` | Opus bitrate |
| `SovEmu__Sessions__IdleTimeoutSeconds` | `300` | Seconds a session survives with no client |
| `SovEmu__Sessions__StartTimeoutSeconds` | `120` | Seconds a game may take to load (disc images on a slow share) |
| `SovEmu__Sessions__StopTimeoutSeconds` | `15` | Seconds a game may take to write its saves and exit before it is killed |
| `SovEmu__Sessions__Rewind__Seconds` | `60` | Rewind history (0: rewind off). A session turns rewind off by itself where a save state is too large or too slow to take every other frame (PS2) |
| `SovEmu__Sessions__Rewind__MaxMegabytes` | `64` | Memory cap for one session's rewind history |
| `SovEmu__Sessions__Limits__MemoryMegabytes` | per system | The most memory one game may hold (resident and swapped out), MiB, before it saves and ends (0: no limit). Unset, each system has SovEmu's choice, several times what its games were measured to use (docs/research/worker-limits-2026-11.md): 1024 for the 2D systems and the PlayStation, 2048 for the N64 and Dreamcast, 4096 for the GameCube and PS2. A game that goes over it writes its battery save and auto state and ends, and shows on the admin Sessions page as over its limit; one that cannot (stuck inside the core) is stopped at 125% of it. `SovEmu__Systems__<id>__Limits__MemoryMegabytes` wins |
| `SovEmu__Sessions__Limits__Cpus` | per system | The CPU time one game may use, in CPUs (`1.5`: a core and a half; 0: no limit): 2 for the 2D systems, none for the 3D ones, which render on the CPU without a GPU. Needs session cgroups (`…__Limits__Cgroups`) |
| `SovEmu__Sessions__Limits__Threads` | per system | The threads one game may run (0: no limit): 64 for the 2D systems, 256 for the 3D ones. Needs session cgroups |
| `SovEmu__Sessions__Limits__OpenFiles` | `1024` | The files one game may have open (0: as many as the server may) |
| `SovEmu__Sessions__Limits__Cgroups` | `true` | Give each game a cgroup of its own, for the CPU and thread limits and the memory one, when the server's cgroup is delegated to it (writable by its user: a systemd unit with `Delegate=yes`, rootless Podman). A stock Docker or Kubernetes container has none, and the memory and open-file limits hold without one |
| `SovEmu__Sessions__FastForward` | `4` | Fast-forward speed |
| `SovEmu__Streaming__Video__KeyframeInterval` | `600` | Frames between video keyframes |
| `SovEmu__Streaming__Video__DropoutKeyframeMilliseconds` | `700` | A client that has been silent this many milliseconds and is then heard again was cut off (a Wi-Fi dropout). It is sent a keyframe at once, and the bandwidth estimate starts again from what it was before, so the picture returns as the link does; left to itself a client waits a second or more before asking for a keyframe, and the estimate stays collapsed for many seconds. `dropouts` in the `session health` line counts them. 0 leaves it to the client |
| `SovEmu__Streaming__Probe__Milliseconds` | `2000` | How long a new player's connection is checked before their game starts. The game waits and the stream pads the link to see what it carries, then starts at a rate it was seen to carry, instead of at `SovEmu__Streaming__Video__Kbps` and adapting from there (several stalls on a slow link, a soft picture for ten seconds on a fast one). The check ends sooner once the estimate settles. 0 turns it off. |
| `SovEmu__Streaming__Probe__LanMilliseconds` | `750` | The same check for a player on the server's own network, where the link is rarely what limits the stream: shorter, and started near `SovEmu__Streaming__Video__MaxKbps`. |
| `SovEmu__Streaming__Input__LapseMilliseconds` | `500` | How long a game keeps a player's held buttons and sticks held once their client stops refreshing them. Clients refresh held input every 100 ms, so a release that never arrives (the input channel stalled, the app lost its window) lets go this soon instead of holding the button for the rest of the game. Raise it for a lossy remote link, where a burst of lost packets lets go of a held button; the log says `client stopped sending input while holding it` when it happens. 300 to 5000: below 300 one lost refresh would let go, and above 5000 a client that went quiet holds a button too long. Fast-forward, rewind and pause always let go after 500 ms, so a lost release cannot leave a game racing |
| `SovEmu__Streaming__Debug__VideoDropPercent` | `0` | Testing only: every game throws away this share (percent) of its outgoing video packets, retransmissions included, as a lossy link would, to see how a stream recovers. Audio and the connection's own packets always go, and the same packets are lost on every run. The worker logs how many it dropped when the session ends. Leave it at 0 |
| `SovEmu__Streaming__Debug__VideoDelayMilliseconds` | `0` | Testing only: every game holds each outgoing video packet this many milliseconds before sending it, as a distant client's link would, so a retransmission arrives late. Audio and the connection's own packets go at once. Leave it at 0 |
| `SovEmu__Streaming__Debug__VideoDropBurstMilliseconds` | `0` | Testing only: every game's link is cut both ways for this many milliseconds once every 10 seconds, as a Wi-Fi dropout does: nothing is sent and nothing the client sends is heard. Leave it at 0 |
| `SovEmu__Streaming__Debug__VideoLimitKbps` | `0` | Testing only: every game's link carries at most this much video (kbit/s), as a slow internet connection does: packets leave one after another at that rate, video that comes faster waits up to 300 ms, and what would wait longer is thrown away. Audio and the connection's own packets are not limited. 0 is no limit; leave it there |
| `SovEmu__Streaming__Debug__ReplayFrames` | | Testing only: a folder of `frame_NNNNNN.png` (the encoder bench's captures, `scripts/encode-quality-captures.sh`) that every game shows instead of its own picture, the next frame each time it draws, looping, so a client decodes exactly the frames the bench scored. Leave it unset |
| `SovEmu__Streaming__Debug__VideoJitterMilliseconds` | `0` | Testing only: every game holds each outgoing video packet a different time, from nothing up to this many milliseconds (on top of `SovEmu__Streaming__Debug__VideoDelayMilliseconds`), without putting any out of order, as Wi-Fi and busy routes do. The same on every run. Leave it at 0 |
| `SovEmu__Sessions__HealthSeconds` | `10` | Seconds between each game's `session health` log line while someone plays (0: off; the `session summary` at the end stays). Both lines count the keyframes sent, the client's requests for one and how many were ignored because a keyframe was already on its way (`keyframes`, `keyframe_requests`, `keyframe_requests_ignored`), and give the largest frame (`largest_frame_kb`) |
| `SovEmu__Sessions__FailuresKept` | `50` | Failed games the admin Sessions page keeps (a load error, a crash, a worker that had to be killed), with the core's last log lines; 0 keeps none. In memory: the log has every one |
| `SovEmu__Home__FeaturedCount` | `8` | Games in the home screen's hero (0 hides it) |
| `SovEmu__Home__RowLength` | `20` | Games in each home screen row (recently played, recently added, top rated) |
| `SovEmu__Rendering__Gpu` | `auto` | 3D rendering: `auto` (first working GPU, else software), `software`, `off`, or a render node (`/dev/dri/renderD128`) |
| **Operations** | | |
| `SovEmu__Http__SecureCookies` | off | Set when served over HTTPS |
| `SovEmu__Network__TrustedProxyNetwork__<n>` | none | Reverse proxies whose `X-Forwarded-For` is believed: an address or range per index (`__0=10.0.0.0/8`) |
| `SovEmu__Jobs__MaintenanceIntervalMinutes` | `60` | Database upkeep period: expired sign-ins and old job history deleted, statistics refreshed, the write-ahead log checkpointed (0: never) |
| `SovEmu__Backups__Directory` | `<data>/backups` | Where backups are kept: one zip file each, holding the database, the signing keys (`server.json`) and every save. It holds password hashes and keys: keep it as safe as the data folder |
| `SovEmu__Backups__IntervalHours` | `0` | A backup every this many hours (0: none on a schedule; the admin's Backups page still makes one) |
| `SovEmu__Backups__Keep` | `7` | Backups kept, the newest (0: every one) |
| `SovEmu__Backups__RestoreFrom` | none | A backup to restore at startup, before the database opens. What it replaces is moved into `<data>/before-restore-<time>/`, never deleted, and each backup is restored once however often the server restarts with this set |
| `SovEmu__Jobs__RetentionDays` | `30` | Days of background-job history kept (every scan, sync, library check, lookup and upkeep run, on the admin Jobs page; 0: kept for ever) |
| `SovEmu__Logging__BufferLines` | `2000` | Lines of the server's log (its own and its game sessions') kept in memory for the admin Logs page; 0 keeps none and turns that view off. Lines go to stderr regardless |
| `SovEmu__ActivityLog__RetentionDays` | `90` | Days of the activity log kept (sign-ins and refusals, games started and ended, admin changes, on the admin Activity page; 0: kept for ever) |
| `SovEmu__ClientErrors__RetentionDays` | `30` | Days of client errors kept (what the apps reported, on the admin Client errors page; 0: kept for ever) |
| `SovEmu__SystemResources__SampleSeconds` | `5` | Seconds between readings of the container's CPU and memory (its cgroup: the server and every game session), for the admin Resources page and the OpenTelemetry gauges (0: not read) |
| `SovEmu__SystemResources__RetentionDays` | `30` | Days of CPU and memory history kept, a row per minute (0: kept for ever) |
| `SovEmu__Assets__SecretKey` | generated, in `data/server.json` | The key signing the file URLs the API hands out (artwork, videos, screenshots, decorations). A secret. Changing it invalidates URLs already handed out |
| `SovEmu__Assets__UrlHours` | `24` | How long a signed file URL is valid (rounded to the hour, so URLs stay cacheable) |
| `TZ` | `UTC` | The time zone of log timestamps, an IANA name (`America/Chicago`); workers follow it. The standard variable, not a `SovEmu__` one, so container tools and cluster injectors (k8tz) that set it just work. The chart's `timezone` sets it |
| `SovEmu__Logging__Format` | `text` | Log lines on stderr: `text`, or `json` (one object per line, with the request's or session's fields) for a log collector; workers follow it |
| `SovEmu__Telemetry__Otlp__Endpoint` | unset | An OpenTelemetry collector's OTLP/HTTP address (`http://otel-collector:4318`): traces and metrics are exported there. Off when unset. The standard `OTEL_*` variables apply on top (`OTEL_SERVICE_NAME`, `OTEL_RESOURCE_ATTRIBUTES`, `OTEL_EXPORTER_OTLP_HEADERS`, `OTEL_METRIC_EXPORT_INTERVAL`) |
| `SovEmu__Decorations__Set` | `sovemu` | The decoration (bezel) set games are framed with: `sovemu` (built in), a set from the decoration folders, or `none` |
| `SovEmu__Decorations__Directories__<n>` | none | More decoration folders (Batocera layout), one per index, before `data/artwork/decorations` and the built-in one |
| `SovEmu__Appearance__Background` | unset (the admin's choice on the Appearance page, else `random`) | The background behind every screen, in every client: `dither`, `lattice`, `screens`, a file's name from the background folders, `random` (a different one each visit) or `none`. Set, it locks the Appearance page |
| `SovEmu__Appearance__BackgroundDirectories__<n>` | none | More background folders, one per index, before `data/artwork/backgrounds`: WebP, PNG or JPEG images, offered by file name (`sunset-beach.png` is "Sunset beach"); one named like a built-in one replaces it. System photos of your own go in `data/artwork/systems/<system id>.webp` |
| `SovEmu__Paths__Web`, `SovEmu__Paths__Worker`, `SovEmu__Paths__BuiltInArtwork` | set in the image | The built web client; the worker binary; the built-in artwork (`/opt/sovemu/artwork`, laid out as `data/artwork`) |
| `RUST_LOG` | `info` | Log filter |
| **compose.nas.yaml only** | | |
| `SOVEMU_NAS_HOST`, `SOVEMU_NAS_PATH`, `SOVEMU_NAS_USER`, `SOVEMU_NAS_PASSWORD` | | The SMB share to mount read-only |
| **compose.gpu.yaml only** | | |
| `SOVEMU_RENDER_GID` | | The host group that owns `/dev/dri/renderD*` |

Game workers start with an empty environment, apart from `PATH`, `HOME`, `TMPDIR`, `LANG` and
`RUST_LOG`. Cores are third-party code, so they never see credentials.

### Lists and maps

Accounts and per-system settings go one level deeper in the same hierarchy:

```bash
SovEmu__Users__0__Username=kid
SovEmu__Users__0__Password=...
SovEmu__Users__1__Username=dad
SovEmu__Users__1__Password=...
SovEmu__Users__1__Role=admin                         # viewer (the default) or admin
SovEmu__Users__Prune=true                             # nobody else keeps an account

SovEmu__Systems__psx__Options__pcsx_rearmed_cd_readahead=512   # libretro core option
SovEmu__Systems__nes__Core=nestopia                  # use this core (if installed)
SovEmu__Systems__gamegear__Enabled=false             # hide a system
SovEmu__Systems__gba__Decoration=none                # no frame around GBA games
SovEmu__Systems__snes__RunAhead=2                   # hide two frames of input lag (default 1)
SovEmu__Systems__gb__Prescale=2                     # send Game Boy pixels as 2×2 blocks too
```

Declared accounts are created at startup and kept in step: a changed password signs that user
out, and the admin flag follows the variable. An unknown system id, or a mistyped setting, stops
the server at startup with the variable's name.

### Kubernetes

The Helm chart that deploys SovEmu is kept with the cluster's other charts, outside this
repository, and turns its values into these variables. What any deployment needs from SovEmu:

- every setting as an environment variable, secrets included (from a Secret);
- the library mounted read-only (`/sov-library`, or `SovEmu__Paths__Library`), and a persistent
  `/sov-data`;
- the media ports (`SovEmu__Streaming__Udp__Ports`, UDP) reachable at `SovEmu__Streaming__AdvertiseAddress`: a UDP
  `LoadBalancer` Service (MetalLB at home), or host networking (which also offers games over
  the node's IPv6 address, where it has one);
- for players away from home, the built-in relay's port, 3478, over UDP and TCP. With host
  networking the server sees the LAN and asks the router to forward it, and the session ports,
  itself (`SovEmu__Network__PortMapping`); behind a `LoadBalancer` the router's search doesn't
  reach it, so forward them by hand;
- for GPU rendering and encoding, the render node (`/dev/dri`), or NVIDIA's container runtime.

The image runs as uid 1000 and works with a read-only root filesystem and no capabilities. On
SIGTERM the server stops every session cleanly before exiting, so saves are written.

CI publishes the image to `ghcr.io/sovereign-media/sovemu` (a private package) on every merge to
`main`, once the test suite and the core probe have passed: `bleeding` follows `main`, and
`bleeding-<UTC yyyymmddHHMMSS>` pins one build and sorts by time, so an updater can track
`glob:bleeding-*` (Keel). Pull it with an `imagePullSecrets` entry holding a token with
`read:packages`.

### Health and sign-in

`GET /health` answers `ok` while the database is reachable. Sign-in locks a username out after
`SovEmu__Authentication__Lockout__MaxFailures` failed attempts, and a client address after
`SovEmu__Authentication__Lockout__MaxFailuresPerAddress` (across usernames), for `SovEmu__Authentication__Lockout__Minutes`.
Behind a reverse proxy every request comes from the proxy, so list it in
`SovEmu__Network__TrustedProxyNetwork`: its `X-Forwarded-For` then names the client (the last address in it
that is not a listed proxy, so a client cannot choose its own). Anyone else's
`X-Forwarded-For` is ignored. Responses are compressed (Brotli or gzip) for clients that accept
it, except images and videos.

### Playing away from home

Expose 8080 through an HTTPS reverse proxy (and set `SovEmu__Http__SecureCookies=1`). A game's
picture then needs a path from the player to its session, one of:

- **With IPv6, nothing at all.** Where the server has a global IPv6 address (on the host's own
  network, not a container's bridge), each game is offered at it too, on the same port, and a
  player with IPv6 connects to it directly: there is no NAT on the way. If the router's IPv6
  firewall blocks what comes in, both sides' checks open it as they do for IPv4. Players without
  IPv6 use IPv4 as below.
- **Over IPv4, usually nothing.** Each game asks Cloudflare's and Google's free STUN servers
  what address your router gives it, and offers that to players beside its own. The player's
  device does the same. Both send to each other, and most home routers then let the stream
  through with no port forwarded.
- **The router forwards the ports itself.** At startup the server asks the router, over UPnP or
  NAT-PMP, to forward the session ports (50000-50009/udp) and the relay's port (3478, UDP and TCP)
  to it, and each game offers the public address the router forwards beside STUN's. That gets a
  direct path even through a router that gives every destination its own port (a symmetric NAT),
  and makes the relay reachable with nothing forwarded by hand. The mappings are renewed while the
  server runs and removed when it stops. Most routers have UPnP on; where it is off, or the
  server runs in a container on a bridge network (the search can't leave it), the log says no
  router answered and the server carries on. The admin Network page shows what the router
  forwards. `SovEmu__Network__PortMapping=false` turns it off.
- **Otherwise, the built-in relay**: forward port 3478, over UDP and TCP, to the server, if the
  router didn't. It is always on, and it carries the stream for players no direct path reaches:
  behind a symmetric NAT the router won't forward through, behind carrier-grade NAT (the log
  warns when the router's own address is a private one), or on a network that blocks UDP (TCP
  3478 still gets through many). `SovEmu__Streaming__AdvertiseAddress` stays the LAN address.

Neither STUN nor the relay has a setting: there is nothing to get wrong. Each signed-in player
gets relay credentials of their own in `clientConfig`, so there is no relay password, and the relay sends
only to this server's game sessions, never anywhere else on your network. It is handed out at the
host the player reached the server at, and it is the only TURN server SovEmu uses. A relayed
player costs the server about 3 Mbit/s each way. If something else already has port 3478, the
server starts without the relay and logs a warning. A STUN server sees this server's (and a
player's) address and when a game starts, never the game itself. TURN over TLS on 443 isn't
offered: it isn't HTTP, so the HTTPS proxy can't carry it.

Forwarding the session ports (50000-50009/udp) and setting `SovEmu__Streaming__AdvertiseAddress`
to your public address also works, but players on your LAN are then sent to the public address
too, which needs a router that supports hairpin NAT.

## Building from source

You need:
- Rust (stable, 1.89+);
- libclang, for bindgen;
- libvpx, opus, FFmpeg (libavcodec, libavutil) and pkg-config;
- Node 22.23.4+ for the web client.

On macOS: Xcode Command Line Tools and `brew install libvpx opus ffmpeg pkgconf`. On
Debian/Ubuntu: `libclang-dev libvpx-dev libopus-dev libavcodec-dev libavutil-dev pkg-config`.

```bash
(cd SovEmu.Web && npm ci && npm test && npm run build)
cd SovEmu.Backend
cargo nextest run --workspace     # or cargo test --workspace (slower); cargo install cargo-nextest --locked
cargo build --release -p sovemu-worker
cargo build --release -p sovemu-server
target/release/sovemu-server --data-dir ../data --cores-dir ../cores --web-dir ../SovEmu.Web/dist
```

Turn on the pre-push hook once per clone: `git config core.hooksPath .githooks`. Before a push
leaves your machine, it runs the checks for whatever the push changes:
- **`SovEmu.Backend/`:** `cargo fmt --check`, clippy with `-D warnings`, and the tests.
- **`SovEmu.Web/`, or the server's `schema.graphql`:** the schema types check, lint, the
  tests and the build (which checks every GraphQL document against the schema).
- **Docs or CI:** nothing.

Test coverage: `scripts/coverage.sh` (needs `cargo-llvm-cov`: `rustup component add
llvm-tools-preview && cargo install cargo-llvm-cov --locked`) writes a summary, an lcov file and
an HTML report to `coverage/`; `--open` opens the report, `-p <crate>` limits it to one crate.
CI's Coverage job publishes the same for `main`, measured in the container.

On a failure it names the step and gives the command to rerun it. `git push --no-verify` skips
it. CI lints the backend on Linux and runs its tests once, inside the container build, against
the libraries the image ships; nothing in CI runs on macOS.

## Running a core headless

```bash
scripts/fetch-cores.sh fceumm
SovEmu.Backend/target/release/sovemu-worker headless \
  --core cores/fceumm_libretro.dylib \
  --rom /path/to/game.nes \
  --frames 600 --png-every 60 --hold 200-205:start \
  --out headless-out
```

This writes `frame_NNNNNN.png`, `last.png`, `audio.wav` and `summary.json` (core info, A/V
timing, per-frame core time) to `headless-out/`. `--realtime` paces at the core's frame rate;
`--var key=value` sets a core option. Run `sovemu-worker headless --help` for everything else.

Cores, ROMs and BIOS files are never committed.

## Licence

SovEmu is free software under the [GNU Affero General Public License v3.0 or later](LICENSE)
(`AGPL-3.0-or-later`). You may use, change and share it; if you run a modified version as a
service, you must offer its users the source of your version. The admin About page links the
source (`buildInfo.source`). The emulator cores it ships, the game-information services and the
open-source packages it is built from keep their own licences, listed on the About page.

Some of those cores (Snes9x, Genesis Plus GX, FinalBurn Neo, and PicoDrive under the old MAME
licence) forbid commercial use. The image that bundles them may be shared and run for free, but
not sold or used in a paid service; the About page marks each such core.
