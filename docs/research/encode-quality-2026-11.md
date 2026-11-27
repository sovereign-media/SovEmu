# A 2× pre-scale and VP8 screen-content mode on the low-end systems

Measurements for sovereign-media/SovEmu#263, taken on 2026-11-26: whether scaling each frame up
2× before encoding (test 1) or turning on libvpx's screen-content mode (test 2) is worth
shipping, scored on the server for five systems and checked on a Fire TV Stick. The raw numbers,
charts and every clip's capture command are in [`encode-quality-2026-11/`](encode-quality-2026-11/).

## Summary: ship the 2× pre-scale for the colour systems; leave screen-content mode off

**A 2× pre-scale removes almost all of the colour error VP8's 4:2:0 chroma causes on NES, SNES,
Game Boy Color and GBA, and the Fire TV Stick decodes it with room to spare.** At the session's
starting rate of 5,000 kbit/s, the share of pixels that visibly differ from what the core drew
(CIEDE2000 above 2.3) falls from 13–30% to 0.5–10%, and the mean colour error by 50–85%, with the
worst 1% of frames better on every clip. It costs little: VP8 sends at most about twice the bits
at 2× (and fewer on GBA), because these pictures are far cheaper than the rate it is given. An encode
takes under 1 ms a frame at four threads, and the stick decodes a 2× frame in 1.9 ms against a
16.7 ms budget. On the TV the gain is smaller, because **the Fire TV app scales the picture
bilinearly**, which blurs both variants; drawing the picture nearest-neighbour is a separate fix,
probably worth as much again.

**Screen-content mode does nothing measurable in modes 0 and 1, and mode 2 makes the worst frames
worse and drops frames at the lower rates.** Don't ship it.

**The original Game Boy gains nothing from either**: its four greys carry no chroma to lose.

Recommendation per system:

| System | Test 1: 2× pre-scale | Test 2: screen-content mode |
|---|---|---|
| NES | **Ship** (visible pixels 13% → 0.5%) | Off |
| SNES | **Ship** (20% → 3.6%) | Off |
| Game Boy Color | **Ship** (30% → 10%) | Off |
| GBA | **Ship** (19% → 2.3%) | Off |
| Game Boy | No change; leave at 1× | Off |
| Genesis, PC Engine, PS1 | Not measured (no ROM here: see Gaps) | Not measured |

Shipping it is a per-system setting in the worker's video path (`SovEmu__Systems__<id>__…`), a
follow-up; `SovEmu__Streaming__Video__Prescale`, added here, applies it to every system and stays
at 1 by default.

## How it was measured

**Clips.** Ten clips of 1,800 frames (30 s), each the core's own frames as lossless RGBA PNGs,
captured once with `scripts/encode-quality-captures.sh` (the command for each is in
[`commands/`](encode-quality-2026-11/commands/)) and replayed for every variant: NES (*Nova the
Squirrel* gameplay, *240p Test Suite* hill-zone scroll and colour bars/colour bleed), SNES (*Castle
Platformer*), Game Boy in DMG mode (240p Test Suite hill zone, gray ramp and grid), Game Boy Color
(*uCity* scrolling its map, 240p Test Suite SMPTE bars and colour bleed) and GBA (240p Test Suite
hill zone, bars and colour bleed). fceumm crops overscan, so the NES clips are 256×224.
Re-capturing gives byte-identical frames.

**Bench.** `SovEmu.Media/examples/encode_quality` encodes each clip through the session's own
`Vp8Encoder` (and `H264Encoder` forced to x264, for reference only: VP8 is the codec SovEmu
ships), changing only the variable under test, decodes every frame, and scores what the viewer
would see against what the core drew:

- the reference is the core's RGBA frame, before any colour conversion;
- everything is scored on a grid of twice the native size: the reference and a 1× picture
  pixel-duplicated (as the web client's `pixelated` display does), a 2× picture as it is;
- decoded YUV goes back to RGB through the exact inverse of `yuv.rs`, chroma upsampled by nearest
  neighbour for every variant alike (a real display upsamples bilinearly: see the Fire TV's
  screen below);
- a frame rate control drops is scored as the previous picture staying on screen.

Per frame: CIEDE2000 mean, share of pixels above 2.3 and 99th percentile; PSNR on Y, Cb and Cr;
SSIM on luma (8×8 windows every 4 pixels). Per clip: the mean over frames, and the mean of the
worst 1% of frames. The CIEDE2000 implementation matches Sharma, Wu and Dalal's published test
pairs. **Floors**: the same clips through RGBA → I420 → RGB with no codec; the 2× floor is
ΔE2000 0.09–0.34 with no visible pixels, which shows the harness's conversion is right.

**Machine**: Apple M4 Pro (12 cores), macOS 27.0.1, libvpx 1.17.0, libavcodec 63.1.102, Rust
1.100.0, harness at commit `75ca45d` (`feat/encode-quality`).

## Test 1: the 2× pre-scale

VP8, mean over each system's clips (two per system; three on NES), 1× → 2×:

| System, rate | ΔE2000 mean | Pixels > 2.3 | Worst 1% of frames | PSNR-Cb (dB) | Actual kbit/s | Encode p95 (ms) | Decode p95 (ms) | Dropped |
|---|---|---|---|---|---|---|---|---|
| NES, no codec | 1.96 → 0.21 | 12.4% → 0.0% | 3.90 → 0.36 | 35.1 → 63.6 | | | | |
| NES 5,000 | 2.08 → 0.32 | 13.2% → 0.5% | 4.03 → 0.54 | 34.6 → 55.7 | 596 → 610 | 0.42 → 0.83 | 0.24 → 0.30 | 0 → 0 |
| NES 1,500 | 2.09 → 0.36 | 13.5% → 1.2% | 4.15 → 0.92 | 34.5 → 54.9 | 594 → 613 | 0.43 → 0.94 | 0.24 → 0.30 | 0 → 0 |
| SNES, no codec | 1.96 → 0.86 | 15.0% → 0.0% | 2.33 → 0.95 | 40.9 → 59.1 | | | | |
| SNES 5,000 | 2.14 → 1.03 | 20.4% → 3.6% | 2.57 → 1.38 | 40.7 → 54.2 | 657 → 824 | 0.44 → 0.72 | 0.22 → 0.28 | 0 → 0 |
| SNES 1,500 | 2.14 → 1.07 | 20.5% → 4.4% | 2.64 → 2.58 | 40.7 → 53.7 | 660 → 846 | 0.37 → 0.75 | 0.22 → 0.29 | 0 → 0 |
| GBC, no codec | 2.91 → 0.38 | 20.3% → 0.0% | 5.49 → 0.64 | 34.4 → 62.7 | | | | |
| GBC 5,000 | 3.21 → 0.77 | 29.6% → 10.0% | 5.96 → 1.36 | 34.3 → 54.0 | 239 → 417 | 0.23 → 0.61 | 0.11 → 0.25 | 0 → 0 |
| GBC 1,500 | 3.21 → 0.89 | 29.6% → 11.6% | 6.00 → 2.65 | 33.7 → 52.9 | 238 → 414 | 0.30 → 0.58 | 0.11 → 0.23 | 0 → 0 |
| GBA, no codec | 2.86 → 0.26 | 17.7% → 0.0% | 5.64 → 0.36 | 28.7 → 61.4 | | | | |
| GBA 5,000 | 3.05 → 0.47 | 19.4% → 2.3% | 5.86 → 0.69 | 28.6 → 54.0 | 443 → 337 | 0.29 → 0.55 | 0.15 → 0.15 | 0 → 0 |
| GBA 1,500 | 3.06 → 0.51 | 19.7% → 3.4% | 5.87 → 2.41 | 28.6 → 53.7 | 445 → 369 | 0.29 → 0.65 | 0.15 → 0.18 | 0 → 0 |
| GB, no codec | 0.40 → 0.34 | 0.3% → 0.0% | 0.53 → 0.47 | 60.5 → 64.5 | | | | |
| GB 5,000 | 0.54 → 0.49 | 2.8% → 2.5% | 0.68 → 0.61 | 58.2 → 59.2 | 109 → 222 | 0.16 → 0.44 | 0.04 → 0.11 | 0 → 0 |
| GB 1,500 | 0.54 → 0.50 | 2.7% → 2.8% | 0.69 → 0.67 | 58.2 → 59.2 | 110 → 228 | 0.15 → 0.46 | 0.04 → 0.12 | 0 → 0 |

What the table shows:

- **The 1× floor is most of the 1× error.** With no codec at all, 4:2:0 already leaves 12–20% of
  pixels visibly wrong on the colour systems; VP8 adds little on top. The 2× floor has none.
- **2× wins at every rate, not just the default**: see the curves in
  [`charts/`](encode-quality-2026-11/charts/) (`prescale-<system>.svg`, ΔE2000 against actual
  bitrate; `-visible.svg`, pixels above 2.3). At 800 kbit/s the gain is the same shape.
- **The cost in bits is small, and sometimes negative.** None of these clips uses the rate it is
  given: VP8 sends 100–850 kbit/s against targets of 1,500 and 5,000. Each clip's worst 1% of
  frames is better at 2× on every clip at both rates.
- **Game Boy (DMG)** improves only within noise, and one clip's visible share rises
  slightly at 1,500 (1.7% → 2.5%), for twice the bits: not worth it.
- GBC's *uCity* keeps the most error at 2× (36% → 17% visible): its map is all fine dithered
  detail, which VP8 smooths at these rates.

x264 (reference only, not shipped) shows the same picture with larger gains at 2× (visible pixels
on GBA 17.7% → 0.2%, on SNES 15.9% → 0.5%), and fewer bits than VP8 at 2×.

## Test 2: screen-content mode

VP8, modes 0 / 1 / 2, means per system ([`test2-screen-content.csv`](encode-quality-2026-11/test2-screen-content.csv)).
At 1× and at 2×, at every rate, modes 0 and 1 are the same within noise on every system (ΔE2000
mean within ±0.015, visible share within ±0.5 points). Mode 2 is the same as mode 1 at 2,500 and
5,000, and worse at 800 and 1,500 at 2×:

| 2× | Worst 1% of frames, mode 0 / 1 / 2 | Dropped, mode 0 / 1 / 2 |
|---|---|---|
| NES 800 | 1.57 / 1.55 / **6.75** | 0 / 0 / 2 |
| NES 1,500 | 0.92 / 0.92 / **2.52** | 0 / 0 / 1 |
| SNES 1,500 | 2.58 / 2.78 / **3.65** | 0 / 0 / 1 |
| GBA 800 | 2.95 / 2.46 / **5.70** | 0 / 0 / 1 |

Encode times are the same in every mode. In libvpx's real-time VP8 encoder, screen-content mode
changes the stream only when the rate is tight: with the rate to spare these clips have, the
encoder is at its lowest quantizer in every mode.

## On the Fire TV Stick

**Device**: Fire TV Stick, model AFTMA08C15 (MediaTek MT8696, 32-bit), Fire OS 8.1.8.6
(RS8186/3977), on 5 GHz Wi-Fi at a 1,201 Mbit/s link rate (RSSI −55 dBm), to a server on the same
LAN. VP8 is decoded in software (libvpx), as `DecoderLatency.prefersSoftware` chooses on this
chip. App: SovEmu.Android `c3a0d1f` (release, for timing) and `2019005` (`feat/frame-dump`,
debug, for the decoded-frame dumps). Variants: SNES *Castle Platformer* and the NES colour clip,
1× and 2×, at 5,000 and 1,500 kbit/s, the rate pinned (`Kbps`, `MinKbps` and `MaxKbps` equal).

**How**: `SovEmu__Streaming__Debug__ReplayFrames` makes the worker show the captured clip instead
of the game's picture, frame *i* at tick *i*, so the stick decodes exactly the frames the bench
scored through the real server, worker, network and app. The debug app's frame dump writes each
decoded frame's I420, and `encode_quality score-dump` scores it with the bench's own scoring,
placing each dumped frame after the one before it (the receiver's frames carry no timestamp on
Android, and arrival times are too bursty to place frames by) and checking it against the
references nearby.

### The decoded frames match the server

| Clip, rate | ΔE2000 mean, server / stick | Pixels > 2.3, server / stick | Worst 1%, server / stick | Frames lost |
|---|---|---|---|---|
| SNES 1× 5,000 | 2.136 / 2.131 | 20.4% / 20.1% | 2.57 / 2.57 | 0 |
| SNES 2× 5,000 | 1.033 / 1.039 | 3.6% / 3.6% | 1.38 / 1.98 | 1 |
| SNES 1× 1,500 | 2.142 / 2.140 | 20.5% / 20.4% | 2.64 / 2.63 | 0 |
| SNES 2× 1,500 | 1.071 / 1.075 | 4.4% / 4.5% | 2.58 / 2.58 | 0 |
| NES colour 1× 5,000 | 2.798 / 2.798 | 16.4% / 16.4% | 7.02 / 7.02 | 0 |
| NES colour 2× 5,000 | 0.144 / 0.144 | 0.05% / 0.05% | 0.32 / 0.32 | 0 |
| NES colour 1× 1,500 | 2.794 / 2.794 | 16.4% / 16.4% | 7.03 / 7.03 | 0 |
| NES colour 2× 1,500 | 0.150 / 0.150 | 0.10% / 0.10% | 0.45 / 0.45 | 0 |

The stick's libvpx decodes what the server's does; one frame of 1,800 never arrived, in one 2×
run (its worst 1% rises accordingly).

### On the screen: the app scales bilinearly

`adb exec-out screencap` captures the video surface. The app draws the picture in a 1440×1080
rectangle (4:3, 5.6× the SNES width) on a 1920×1080 surface. A still frame (one frame replayed in
a loop) was captured at 1× and 2× and scored against the reference scaled to that rectangle by
nearest neighbour, the ideal sharp display ([`firetv-screen.csv`](encode-quality-2026-11/firetv-screen.csv)):

| Still frame | ΔE2000 mean, 1× → 2× | Pixels > 2.3, 1× → 2× | Edge width on screen (px), 1× → 2× |
|---|---|---|---|
| SNES gameplay | 2.94 → 1.90 | 27.3% → 16.7% | 14.4 → 10.5 |
| NES colour bars | 0.86 → 0.55 | 4.6% → 2.4% | 10.7 → 7.2 |
| NES colour bleed (one-line stripes) | 12.6 → 6.9 | 58.7% → 42.3% | (no clean edges) |

**A sharp edge between two flat colours spreads over 7–14 screen pixels** where a
nearest-neighbour scale would put it within one: the app scales bilinearly, as the issue suspected
from libwebrtc creating its frame textures with `GL_LINEAR`. 2× still helps on screen, cutting the
error by a third to a half, but every number on screen is higher than in the decoded frames because the
blur adds its own. On the colour-bleed test the difference is plain to see: at 1× the green
stripes on black turn into a grey-green blur, at 2× they stay green.

### What it costs the stick

Release build, dump off, 60 s of SNES gameplay with a button pressed every 2 s, two runs per
variant (three at 1× 5,000), from the `client stats` the app sends the worker every 10 s
([`firetv-timing.csv`](encode-quality-2026-11/firetv-timing.csv)):

| Variant | Decode mean (ms) | Worst 10 s (ms) | Input to frame, median (ms) | Frames dropped | Packets lost | App CPU |
|---|---|---|---|---|---|---|
| 1× 5,000 | 0.91–1.01 | 1.70 | 19–21 | 0 | 0 | 49–52% |
| 2× 5,000 | 1.91 | 2.90 | 22–23 | 0 | 0 | 61–64% |
| 1× 1,500 | 0.99–1.00 | 1.80 | 21 | 0 | 0 | 50% |
| 2× 1,500 | 1.93–1.97 | 2.40 | 23–24 | 2 (one run) | 0 | 57–60% |

A 2× frame takes the stick twice as long to decode, about 2 ms, nowhere near the 16.7 ms a frame
lasts; input to frame grows by 2–3 ms, well under a frame. **Test 1 passes on the stick.**

## Against the issue's criteria

| Criterion | NES | SNES | GBC | GBA | GB |
|---|---|---|---|---|---|
| ΔE2000 mean lower at 5,000 and 1,500 | Yes | Yes | Yes | Yes | Marginal |
| Pixels > 2.3 down by a quarter or more | Yes (−96%) | Yes (−82%) | Yes (−66%) | Yes (−88%) | No |
| Worst 1% no worse | Yes | Yes | Yes | Yes | Yes |
| The crops show it | Yes | Yes | Yes | Yes | No |
| Encode p95 within 5 ms, drops not up | Yes (< 1 ms) | Yes | Yes | Yes | Yes |
| Fire TV: decode p95 under 16.7 ms, drops and latency | Yes (measured on SNES) | Yes | Not run | Not run | — |
| Fire TV screen: lower error | Yes | Yes | Not run | Not run | — |

Screen-content mode fails "pixels > 2.3 down by a quarter" on every system in every mode.

## Findings beyond the two tests

- **The Fire TV app draws the picture bilinearly.** Drawing it nearest-neighbour (as the web
  client's `pixelated` does) is likely worth about as much as the pre-scale on a TV, and is a
  separate change in SovEmu.Android.
- **VP8's output depends on the machine's load.** With a positive `cpu_used` (the session's is
  8), libvpx's real-time VP8 encoder changes speed with how long frames take, so the same frames
  make different bytes from run to run: repeat runs differed by up to 19% in size on one clip
  (SNES 1× 5,000, whose first run also had a 14.5 ms encode p95). The quality metrics moved far
  less than the bitrate, and the conclusions don't depend on it, but the issue's "check two runs
  match" does not hold for VP8 at `cpu_used` 8. A bench that must repeat exactly can use a
  negative `cpu_used` (a fixed speed).
- x264 also differed slightly between runs, by up to 6% in size.
- **The rate is mostly unused.** At 1,500 and 5,000 kbit/s, every clip sends far less; the
  adaptation range (300–12,000) matters for loss and congestion, not for these pictures.

## Gaps

- **Genesis, PC Engine and PS1 were not measured**: there's no freely licensed ROM for them in
  `scripts/fetch-free-roms.sh`, and Artemio's 240p Test Suite publishes only sources for them
  (built with SGDK and HuC). `mednafen_pce_fast` and `genesis_plus_gx` are fetched; a run needs
  only clips (`scripts/encode-quality-captures.sh` takes a new line each).
- On the stick, only SNES and NES clips were run; GBC and GBA weren't (they are smaller frames,
  so cheaper to decode).
- Fire TV decode times are WebRTC's mean per 10 s window, not per-frame percentiles (the app's
  per-frame decode time isn't exposed), so "decode p95" above is the worst window.
- The client's reported bitrate (≈4,000 kbit/s at a 5,000 target, against the ≈650 the encoder
  makes) appears to count the connection's bandwidth-probe padding; the worker's `video_kbps`
  agrees with the bench.
- The iOS player's display filter wasn't checked.
- VMAF wasn't computed (not a headline metric for pixel art).
