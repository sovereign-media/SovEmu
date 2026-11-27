# A GPU core's frame, from its framebuffer to the encoder

Measurements for sovereign-media/SovEmu#283, taken on 2026-11-27: what a 3D core's frame costs
between `retro_run` and the video encoder at an N64's and a GameCube's 2× and 4× resolutions,
before and after converting it to I420 on the GPU.

## Summary: frames reach the encoder four times sooner at GameCube 4×

**Converting on the GPU takes the worker's per-frame work before the encoder from 25.5 ms to
6.2 ms at GameCube 4× (2560×2112), and from 4.8 ms to 1.7 ms at N64 4× (1280×960).** At 60 fps a
frame has 16.7 ms, so before this change the worker could not keep up at GameCube 4× even before
encoding. VP8's own encode call also gets faster (13.5 ms to 9.1 ms at GameCube 4×), because it
no longer converts RGBA itself.

Where the time went, and what changed:

- **Main's RGBA copy was the largest single cost**: 12.6 ms a frame at GameCube 4×. The copy
  only sets alpha to 255, but it pushed one pixel at a time into a growing buffer.
  `VideoFrame::write_rgba8` now sizes the buffer once and makes one pass the compiler
  vectorises: 1.2 ms. This helps every core, 2D ones included.
- **The CPU's RGBA → I420 conversion** (4.7 ms at GameCube 4×) is gone for a GPU core: a
  fragment shader does it, and its output matches the CPU's byte for byte.
- **Less to read back**: I420 is 1.5 bytes a pixel against RGBA's 4, and the shader does the
  vertical flip that was a CPU pass. Read back drops from 8.1 ms to 5.8 ms at GameCube 4×, and
  that figure includes waiting for the core's own drawing to finish (below).

## Results

Milliseconds a frame, the mean of 120 frames after 10 warm-up frames, VP8 at its defaults
(4 threads). The second of two runs: the first differed by at most 0.7 ms in any cell, and by
less than 0.3 ms on the I420 path.

| Frame | Path | Read back | Keep | To I420 | VP8 encode (with its conversion) | Frame to encoder input |
|---|---|---:|---:|---:|---:|---:|
| N64 2× (640×480) | RGBA, main's copy | 1.17 | 0.24 | 0.29 | 1.57 | 1.70 |
| N64 2× (640×480) | RGBA | 1.36 | 0.07 | 0.28 | 1.63 | 1.71 |
| N64 2× (640×480) | I420 on the GPU | 0.74 | 0.01 | 0.01 | 0.87 | 0.76 |
| N64 4× (1280×960) | RGBA, main's copy | 1.86 | 1.91 | 1.02 | 3.77 | 4.79 |
| N64 4× (1280×960) | RGBA | 1.86 | 0.27 | 1.03 | 3.78 | 3.16 |
| N64 4× (1280×960) | I420 on the GPU | 1.63 | 0.03 | 0.04 | 2.80 | 1.70 |
| GameCube 2× (1280×1056) | RGBA, main's copy | 2.06 | 0.92 | 1.13 | 4.14 | 4.11 |
| GameCube 2× (1280×1056) | RGBA | 2.17 | 0.30 | 1.13 | 4.12 | 3.61 |
| GameCube 2× (1280×1056) | I420 on the GPU | 1.74 | 0.04 | 0.04 | 3.08 | 1.82 |
| GameCube 4× (2560×2112) | RGBA, main's copy | 8.13 | 12.60 | 4.78 | 13.45 | 25.51 |
| GameCube 4× (2560×2112) | RGBA | 7.89 | 1.23 | 4.65 | 13.51 | 13.77 |
| GameCube 4× (2560×2112) | I420 on the GPU | 5.83 | 0.18 | 0.24 | 9.13 | 6.24 |

- **Read back**: from the end of `retro_run` to the frame in system memory
  (`FrameTiming::readback`): waiting for the GPU to finish the core's drawing, then
  `glReadPixels` and the flip (RGBA), or the I420 pass, its read and packing its rows into planes.
- **Keep**: the stream loop keeping the frame as the picture to encode: the RGBA copy, or the
  I420 bytes as they are.
- **To I420**: what an encoder does before it encodes: `rgba_to_i420`, or a copy of each plane.
- **VP8 encode**: the whole `Vp8Encoder::encode` (or `encode_i420`) call, which converts again
  inside it, so it is not added to the last column.
- **Frame to encoder input**: read back + keep + to I420, the worker's work for a frame before
  the codec's.

## How it was measured

`SovEmu.Worker/tests/gpu_frames.rs`, an ignored test that prints the table:

```bash
cd SovEmu.Backend
cargo test --release -p sovemu-worker --test gpu_frames -- --ignored --nocapture
# SOVEMU_BENCH_GPU=/dev/dri/renderD128 (or software) picks the GPU; auto by default
```

It runs `SovEmu.TestCore` in its sized GL modes (`sovemu_test_render=gl-2560x2112` and so on),
through `Core::run_frame` as a session does, along three paths: main's RGBA path (its old copy
loop is kept in the bench), the RGBA path as it is now, and `Core::set_gpu_i420`. The test core's
frame is two clears, so its own drawing costs almost nothing; what it draws does not change these
costs, which go by the number of pixels.

**Machine**: Apple M4 Pro, Docker Desktop's Linux VM (12 CPUs, 7.7 GiB, kernel 7.0.14), an arm64
Debian 13 container with Rust 1.100 and Mesa from Debian, run natively (no x86 emulation). No GPU:
the frames are drawn and converted by **llvmpipe**, Mesa's software renderer.

**Not measured with real N64 or GameCube games.** The cores exist only for x86_64, which runs here
under emulation, and #282 found the 240p Test Suite does not run in either core that way
(Mupen64Plus-Next spins; Dolphin cannot make its shader context under llvmpipe). The costs here
depend on the frame's size, not its content, so the test core's frames stand in for them.

## What a hardware GPU changes

On llvmpipe the shader runs on the CPU, spread over every core, so the gain above is mostly CPU
work avoided. On a GPU the pass costs a fraction of a millisecond, and reading back 1.5 bytes a
pixel instead of 4 matters more: a discrete GPU reads back over PCIe, and an integrated one through
memory the CPU reads uncached. **These numbers have not been taken on a GPU yet.** The bench takes
a render node (`SOVEMU_BENCH_GPU`) so they can be, on the Alder Lake server.

## Not done, and why

- **Zero-copy to VA-API**, which #283 first proposed: exporting the framebuffer as a dmabuf into
  a VA-API surface so the frame never reaches the CPU. It would help only H.264 on VA-API, but
  SovEmu streams VP8 (libvpx encodes on the CPU and needs the frame in memory), and the server's
  encoder test drops VA-API for x264 where it holds a frame back.
- **Reading back asynchronously** (a pixel-buffer object, mapped later): a frame cannot be
  encoded before it is read, and reading it a frame later would add a frame of latency. Nothing
  else in the stream loop needs the CPU while it waits, so there is nothing to overlap the wait
  with. Worth looking at again on a GPU, if waiting turns out to be most of the read-back column.
- **Pre-scaled 3D frames stay RGBA**: the 2× pre-scale (`SovEmu__Streaming__Video__Prescale`)
  works on RGBA, so a session with it on asks for RGBA frames. No 3D system pre-scales by default.
