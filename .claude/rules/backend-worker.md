---
paths:
  - "SovEmu.Backend/SovEmu.LibretroFfi/**"
  - "SovEmu.Backend/SovEmu.RetroHost/**"
  - "SovEmu.Backend/SovEmu.TestCore/**"
  - "SovEmu.Backend/SovEmu.Media/**"
  - "SovEmu.Backend/SovEmu.Worker/**"
---

# Cores, encoding and the worker

The crates that run a libretro core and stream it: `LibretroFfi` → `RetroHost` → `Worker`, with
`Media` (codecs) and `Protocol` (wire formats). Only `sovemu-worker` links them.

## One core per process

- Cores keep their state in globals and their callbacks carry no user data, so `RetroHost` keeps
  process-global callback state and `Core::load` refuses a second core. **Never try to run two
  sessions in one process**: the server starts one `sovemu-worker session` per game, so a
  crashing core only kills its session.
- **Never hold the callback-state lock across a call into the core.** The core calls back into us
  from inside `retro_run`, `retro_load_game` and `retro_set_environment`; holding the lock there
  deadlocks. `callbacks::with` is for short reads and writes only.
- **FFI callbacks** are `unsafe extern "C" fn`s in `RetroHost/src/callbacks.rs`. A panic inside one
  aborts the process: acceptable in a per-session worker, but never relied on; return an error
  value where the API has one.
- **Environment commands**: an unsupported one returns `false` (logged once at debug). Adding one
  means handling it in `HostState::environment`, covering it in `SovEmu.TestCore` if the test core
  can exercise it, and testing it in `RetroHost/tests/test_core.rs`.
- **Never edit `SovEmu.LibretroFfi/include/libretro.h`** or the generated bindings. Update the
  header from upstream and record the commit in `include/README.md`. Higher crates use
  `RetroHost`'s safe API, never `LibretroFfi` directly (`TestCore` uses its types only).

## GPU rendering

Headless EGL in the worker (`RetroHost/src/gl.rs`): libEGL is opened at run time, never linked, so
the worker still runs without Mesa. A GPU core's frames are read back from a framebuffer object (its
colour a texture) after `retro_run`: as RGBA, or, when the stream asks (`Core::set_gpu_i420`),
converted to I420 by a shader in a second context sharing the core's (`gl/yuv.rs`), which never
touches the core's GL state and must match `Media`'s conversion byte for byte. If that pass fails
its check, frames stay RGBA, so every consumer takes either format. The core's context is
current on the thread that runs the core and must be made current before any call into it. `sovemu-worker gpu` is how the server learns about GPUs: a device
counts only if it drew a frame and read it back. A GPU with no DRM node (NVIDIA without
`nvidia-drm`) is reported as `auto`, resolved hardware-first. Vulkan cores are not supported yet.

## Saves

**Never lose a player's progress.** Battery saves and states are written atomically (temp file,
`fsync`, rename, `fsync` of the directory) in `persist.rs`, on every exit path (`Session::finish`,
called from `Drop`, which waits until they are on the disk). Never write a save in place. Writes
run on the saver's thread, never the stream's: copy what the core hands over, then queue it.

## Encoding

`Media` knows nothing about libretro: packed pixels (RGBA, XRGB8888, RGB565, 0RGB1555, its own
`PixelLayout`) → I420/NV12 straight from the core's format, SSE2 or AVX2 picked at run time and
matching its scalar reference (kept in its tests) byte for byte, H.264 (libavcodec: VA-API, NVENC or x264,
picked at run time, falling back to x264), VP8 (libvpx) and Opus (libopus), all tuned for real
time, plus `AudioEncoder` (any core rate to 48 kHz, 10 ms packets). Latency comes before quality.

## The worker

Subcommands: `session` (one game driven by the server over JSON lines on stdin/stdout,
`SovEmu.Protocol::ipc`, streaming over WebRTC with str0m on one UDP port), `serve` (the same loop
with its own HTTP, for development and tests), `headless` (PNG frames, a WAV, a timing summary),
`probe` (cores' info as JSON), `gpu`, `encoders` (times every video encoder, the server's startup
test) and `connection-test` (answers one browser's offer and echoes its data channel).

The main loop is `stream.rs`; `udp.rs` the session's sockets (IPv4, and IPv6 on the same port
where the host has a global address); `video.rs` the codec per client; `session.rs` the IPC; `persist.rs`
saves; `patch.rs` soft patches; `rewind.rs`, `runahead.rs`, `pacing.rs`, `schedule.rs`,
`adapt.rs`, `loss.rs` and `keyframes.rs` timing and adaptation; `watchdog.rs` and `health.rs`
liveness; `standin.rs` the stand-in games that make option-on-load cores declare their options.

A worker gets **no secrets**: it starts with a cleared environment (see
[backend-server.md](backend-server.md)), so whatever it needs is an argument.
