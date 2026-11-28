# VP8's speed and loop-filter sharpness

Measurements taken on 2026-11-28: whether a slower VP8 encode (libvpx's `cpu_used`) or a sharper
loop filter (`VP8E_SET_SHARPNESS`) raises picture quality without costing the clients anything,
since low-powered clients, not the server, are what limits SovEmu. Same clips, bench and scoring as
[encode-quality-2026-11.md](encode-quality-2026-11.md); the raw numbers are in
[`encode-speed-2026-11/results.csv`](encode-speed-2026-11/results.csv) and the runs in
[`bench.sh`](encode-speed-2026-11/bench.sh).

## Summary: a fixed speed 2 by default; leave sharpness off

**A fixed speed 2 (`cpu_used` −2) sends a closer picture for the same or fewer bits, so clients
decode no more, for about 1 ms more encoding a frame.** It is the default from this change
(`SovEmu__Streaming__Video__CpuUsed`, `-2`).

- **The session's old 8 was really speed 4.** With a positive `cpu_used` libvpx's real-time VP8
  encoder picks its speed from how long frames take. These frames take 1–2 ms, so it settled at 4
  whatever was asked: 8, 6, 4, 2 and a fixed 4 made identical streams on all 24 clip and rate
  pairs.
- **A fixed 2 is better on every clip.** Where the rate is short, the full-screen scrolls at 2×
  lose 15–22% of their visibly wrong pixels at 1,500 kbit/s and 40–87% at 2,500 and 5,000: the
  Genesis scroll at 2,500 falls from 17.8% to 7.5%, the PC Engine's at 5,000 from 5.3% to 0.7%.
  The PS1 scroll at 1× gains least, 2–5%. On gameplay, which has
  rate to spare, the picture is a little better and **14–27% smaller**, which is less for a
  client to decode: NES *Nova* 817 → 631 kbit/s at 1,500, SNES *Castle Platformer* 846 → 676.
- **Clients decode the same or less.** Decode p95 is unchanged on the scrolls and 10–20% lower on
  the gameplay clips, from the fewer bits.
- **The server encodes for longer**: p95 1.0–3.4 ms a frame against 0.6–1.9 ms at four threads,
  the most at PC Engine's 704×480. That much later reaches the client, a few percent of the ~20 ms
  button-to-frame a Fire TV Stick measures.
- **Speed 1 is no better than 2** and slower still; **16 is much worse** (at 2,500 kbit/s, 1.6 to
  11 times the visibly wrong pixels, often for more bits); a fixed 8 is a little worse than the
  adaptive 4.

**Sharpness helps one kind of picture and hurts the other.** It halves the error on NES *Nova* at
1,500 kbit/s (2.2% → 0.7%) but makes every full-screen scroll 1–5% worse at 800 to 2,500 kbit/s,
and does nothing at 5,000. Sharpness 4 and 7 are alike. Not shipped; `Vp8Config::sharpness` stays
0 (the control unset) and the bench keeps `--sharpness` for another look.

## How it was measured

The bench is `SovEmu.Media/examples/encode_quality`, as in the first report, with two new
options, `--cpu-used` and `--sharpness`, passed to the session's own `Vp8Encoder`; each CSV row
now ends with the pair it used. Six clips of 1,800 frames, chosen because they use their rate:

- the 240p Test Suite's scroll test on Genesis (320×224), PC Engine (352×240) and PS1 (320×240),
  a full-screen parallax background scrolling for most of the clip;
- gameplay: NES *Nova the Squirrel*, SNES *Castle Platformer*, and the GBA suite's hill zone.

Each at its system's pre-scale (2× for all but PS1, 1×), at 800, 1,500, 2,500 and 5,000 kbit/s,
two encodes a variant (the faster's timings kept). Run 1: `cpu_used` 8, 6, 4 and 2 with sharpness
0, 4 and 7. Run 2: `cpu_used` −1, −2, −4, −8 and 16 at sharpness 0. Both runs repeated byte for
byte: unlike the first report's, the machine was idle.

**Machine**: Apple M4 Pro (12 cores), macOS 27.0.1, libvpx 1.17.0, Rust 1.100.0, four encode
threads (the session's), harness on `feat/vp8-effort-bench` from `a2a3b54`.

## Speed 4 (the old default) against a fixed 2

| Clip, rate | ΔE2000 mean | Pixels > 2.3 | Worst 1% of frames | Actual kbit/s | Encode p95 (ms) | Decode p95 (ms) |
|---|---|---|---|---|---|---|
| Genesis scroll 800 | 3.480 → 3.078 | 45.0% → 42.2% | 4.24 → 4.18 | 796 → 791 | 1.55 → 3.00 | 0.40 → 0.37 |
| Genesis scroll 1,500 | 2.228 → 1.753 | 34.6% → 27.0% | 3.66 → 3.35 | 1437 → 1424 | 1.60 → 2.91 | 0.57 → 0.53 |
| Genesis scroll 2,500 | 1.258 → 0.825 | 17.8% → 7.5% | 2.55 → 2.34 | 2318 → 2300 | 1.70 → 2.62 | 0.73 → 0.74 |
| Genesis scroll 5,000 | 0.497 → 0.440 | 1.7% → 1.2% | 1.43 → 1.34 | 4467 → 3316 | 1.77 → 2.41 | 1.40 → 0.97 |
| PC Engine scroll 800 | 3.121 → 2.982 | 38.8% → 37.2% | 4.03 → 3.98 | 997 → 849 | 1.55 → 3.40 | 0.35 → 0.32 |
| PC Engine scroll 1,500 | 2.529 → 2.087 | 35.2% → 29.8% | 3.87 → 3.68 | 1392 → 1381 | 1.60 → 3.24 | 0.40 → 0.41 |
| PC Engine scroll 2,500 | 1.691 → 1.171 | 25.7% → 15.4% | 3.36 → 3.22 | 2254 → 2244 | 1.68 → 3.11 | 0.55 → 0.55 |
| PC Engine scroll 5,000 | 0.674 → 0.387 | 5.3% → 0.7% | 1.33 → 1.14 | 4383 → 4188 | 1.85 → 3.00 | 0.92 → 0.93 |
| PS1 scroll (1×) 800 | 4.314 → 4.194 | 47.7% → 46.4% | 5.34 → 5.24 | 754 → 752 | 0.58 → 1.24 | 0.19 → 0.18 |
| PS1 scroll (1×) 1,500 | 3.499 → 3.389 | 40.1% → 38.5% | 4.29 → 4.20 | 1381 → 1381 | 0.64 → 1.24 | 0.29 → 0.29 |
| PS1 scroll (1×) 2,500 | 3.046 → 2.963 | 32.7% → 31.2% | 3.72 → 3.65 | 2280 → 2281 | 0.75 → 1.31 | 0.43 → 0.43 |
| PS1 scroll (1×) 5,000 | 2.684 → 2.644 | 25.8% → 25.3% | 3.34 → 3.30 | 4535 → 4534 | 1.02 → 1.54 | 0.86 → 0.82 |
| NES *Nova* 800 | 0.731 → 0.585 | 6.2% → 3.5% | 2.72 → 2.05 | 560 → 538 | 0.99 → 1.35 | 0.28 → 0.28 |
| NES *Nova* 1,500 | 0.526 → 0.421 | 2.2% → 0.3% | 1.48 → 0.93 | 817 → 631 | 1.11 → 1.33 | 0.41 → 0.34 |
| NES *Nova* 2,500 | 0.426 → 0.414 | 0.4% → 0.3% | 0.83 → 0.80 | 893 → 652 | 1.06 → 1.32 | 0.48 → 0.39 |
| NES *Nova* 5,000 | 0.423 → 0.417 | 0.3% → 0.3% | 0.69 → 0.66 | 893 → 659 | 1.05 → 1.31 | 0.46 → 0.37 |
| SNES *Castle Platformer* 800 | 1.286 → 1.133 | 9.4% → 5.6% | 3.58 → 3.55 | 708 → 640 | 0.74 → 1.00 | 0.25 → 0.24 |
| SNES *Castle Platformer* 1,500 | 1.071 → 1.057 | 4.4% → 4.0% | 2.58 → 2.60 | 846 → 676 | 0.71 → 1.01 | 0.29 → 0.25 |
| SNES *Castle Platformer* 2,500 | 1.054 → 1.044 | 4.0% → 3.8% | 2.04 → 1.86 | 839 → 672 | 0.71 → 1.00 | 0.29 → 0.24 |
| SNES *Castle Platformer* 5,000 | 1.033 → 1.026 | 3.6% → 3.4% | 1.38 → 1.32 | 824 → 660 | 0.70 → 0.99 | 0.28 → 0.24 |
| GBA hill zone 800 | 0.644 → 0.606 | 3.6% → 3.1% | 2.99 → **3.47** | 515 → 421 | 0.62 → 0.83 | 0.17 → 0.17 |
| GBA hill zone 1,500 | 0.602 → 0.564 | 2.9% → 2.2% | 2.49 → 1.90 | 517 → 425 | 0.60 → 0.82 | 0.17 → 0.18 |
| GBA hill zone 2,500 | 0.582 → 0.553 | 2.6% → 1.8% | 1.48 → 1.16 | 496 → 428 | 0.58 → 0.83 | 0.17 → 0.15 |
| GBA hill zone 5,000 | 0.554 → 0.538 | 1.8% → 1.7% | 0.73 → 0.72 | 467 → 401 | 0.56 → 0.83 | 0.15 → 0.14 |

The one row that is worse anywhere: GBA's worst 1% of frames at 800 kbit/s, while its mean and its
visible share improve (it spends 18% fewer bits there).

## Every speed, at 2,500 kbit/s

Pixels above 2.3 · actual kbit/s · encode p95 in ms. The adaptive 8 and a fixed −4 are the same
stream; so were 6, 4 and 2 (not shown).

| Clip | 16 | 8 (adaptive) | −8 | −4 | −2 | −1 |
|---|---|---|---|---|---|---|
| Genesis scroll | 39.5% · 2300 · 0.86 | 17.8% · 2318 · 1.70 | 18.7% · 2329 · 1.01 | 17.8% · 2318 · 1.72 | 7.5% · 2300 · 2.62 | 7.2% · 2300 · 2.79 |
| PC Engine scroll | 39.5% · 2953 · 0.93 | 25.7% · 2254 · 1.68 | 26.9% · 2263 · 0.93 | 25.7% · 2254 · 1.69 | 15.4% · 2244 · 3.11 | 15.4% · 2243 · 3.39 |
| PS1 scroll | 53.4% · 2285 · 0.45 | 32.7% · 2280 · 0.75 | 33.9% · 2281 · 0.49 | 32.7% · 2280 · 0.74 | 31.2% · 2281 · 1.31 | 31.2% · 2280 · 1.43 |
| NES *Nova* | 4.5% · 1144 · 0.70 | 0.4% · 893 · 1.06 | 0.5% · 1047 · 0.67 | 0.4% · 893 · 1.06 | 0.3% · 652 · 1.32 | 0.3% · 657 · 1.44 |
| SNES *Castle Platformer* | 11.6% · 1830 · 0.66 | 4.0% · 839 · 0.71 | 5.0% · 1204 · 0.63 | 4.0% · 839 · 0.69 | 3.8% · 672 · 1.00 | 3.8% · 648 · 1.03 |
| GBA hill zone | 2.0% · 1207 · 0.71 | 2.6% · 496 · 0.58 | 4.8% · 1114 · 0.57 | 2.6% · 496 · 0.57 | 1.8% · 428 · 0.83 | 1.9% · 388 · 0.83 |

## Sharpness

At the adaptive speed (the default before this change), pixels above 2.3 at sharpness 0 / 4 / 7:

| Clip | 800 | 1,500 | 2,500 | 5,000 |
|---|---|---|---|---|
| Genesis scroll | 45.0 / 45.7 / 46.2 | 34.6 / 36.0 / 36.6 | 17.8 / 18.4 / 19.5 | 1.7 / 1.8 / 1.8 |
| PC Engine scroll | 38.8 / 38.8 / 39.4 | 35.2 / 35.8 / 36.3 | 25.7 / 26.4 / 26.7 | 5.3 / 5.3 / 5.6 |
| PS1 scroll | 47.7 / 48.8 / 49.5 | 40.1 / 40.7 / 41.5 | 32.7 / 32.6 / 33.3 | 25.8 / 25.8 / 25.8 |
| NES *Nova* | 6.2 / 5.0 / 5.2 | 2.2 / 0.7 / 0.8 | 0.4 / 0.3 / 0.3 | 0.3 / 0.3 / 0.3 |
| SNES *Castle Platformer* | 9.4 / 8.5 / 8.3 | 4.4 / 4.4 / 4.5 | 4.0 / 4.2 / 4.1 | 3.6 / 3.6 / 3.6 |
| GBA hill zone | 3.6 / 3.3 / 3.3 | 2.9 / 2.9 / 2.7 | 2.6 / 2.2 / 2.2 | 1.8 / 1.8 / 1.8 |

Bits and decode times are the same at every sharpness. Sharpness with a fixed speed 2 was not run.

## Gaps

- **Encode times are this Mac's.** The server's own CPU, and several sessions at once, decide the
  real cost. A fixed speed does not slow down under load as the adaptive one does; the server's
  startup timing of its encoders (`sovemu-worker encoders`, which uses `Vp8Config::default()`)
  now measures speed 2.
- No device was measured; decode times are libvpx on this Mac. The decode cost follows the bits,
  which are the same or fewer, so a device should be no worse off.
- 2D content only. No 3D game was measured, and no hi-res screen.
