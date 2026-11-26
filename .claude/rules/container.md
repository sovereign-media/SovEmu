---
paths:
  - "SovEmu.Backend/Dockerfile"
  - "SovEmu.Backend/docker-bake.hcl"
  - "SovEmu.Backend/about.*"
  - "SovEmu.Backend/SovEmu.Server/build.rs"
  - ".dockerignore"
  - "compose*.yaml"
  - ".github/**"
  - ".githooks/**"
  - "scripts/**"
---

# The image, compose and CI

## One image

**The deliverable is one self-contained Debian (glibc) x86_64 image**, `SovEmu.Backend/Dockerfile`
(context: the repository root), the same for every host: Mesa in the image drives AMD, Intel and
software rendering, and the NVIDIA container toolkit injects NVIDIA's own driver when there is one.
Never split it into per-GPU images.

Stages: `web` (the web client's tests and build, on `node:<ver>-trixie-slim`); `cores`
(`scripts/install-cores.sh` over `scripts/cores.list`, refusing anything but `linux/amd64`);
`ffmpeg` (libavcodec and libavutil from a pinned FFmpeg release, built for H.264 only: the x264,
VA-API and NVENC encoders and the H.264 decoder the tests use, #236); `toolchain`
(`rust:<ver>-trixie` with Debian's libvpx and libopus, the `ffmpeg` stage's libraries, and the
pinned nextest and cargo-about); `notices` (cargo-about with `about.toml`/`about.hbs`, writing
`third-party-notices.json`); `build` (release binaries); `test-base` and `test` (`cargo nextest
run --workspace --profile ci` with Mesa's llvmpipe); `coverage-run` and `coverage`; and
`runtime`. `build` and `test` compile side by side with their own `target/` cache mounts; CI
builds both at once with `docker-bake.hcl`.

**Nothing that changes per commit or per target goes into the compile**, so `test` and the image
share one `build` stage and it runs once (#245). The build stamp (`SOVEMU_REVISION`,
`SOVEMU_BUILT`) is written by `runtime` as `/opt/sovemu/bin/build.json`, and the notices are
copied there too; the server reads both beside its binary at startup. Both bake targets pass the
same secrets: a difference between them would split `build` into two compiles.

`runtime` is `debian:trixie-slim` with the codecs' and GL's run-time libraries (libvpx, libopus,
the `ffmpeg` stage's libavcodec with Debian's x264, libva with Intel's and Mesa's VA-API drivers, libglvnd and Mesa GL/EGL, no
Vulkan drivers yet), `tini`, and what the release's cores link (`libxi6`, `libusb-1.0-0`,
`libgomp1` …). It:

- sets `NVIDIA_DRIVER_CAPABILITIES=graphics,video,utility` and runs as uid 1000;
- keeps its binaries (`sovemu-server`, `sovemu-worker`) in `/opt/sovemu/bin`, the web client in
  `/opt/sovemu/web`, and the built-in artwork (`sovemu-server --write-artwork`) in
  `/opt/sovemu/artwork`;
- writes every byte to the `/sov-data` volume and reads the library from `/sov-library`
  (read-only); nothing else in the container is written;
- exposes 8080/tcp (web + API), 50000-50009/udp (one port per game session,
  `SovEmu__Streaming__Udp__Ports`) and 3478/udp+tcp (the built-in TURN relay, always on);
- runs `sovemu-worker probe` over every core at build time (writing
  `/opt/sovemu/cores/manifest.json`), so a core that cannot load fails the build;
- runs `sovemu-server` (`CMD`).

## Secrets at build time

SovEmu's ScreenScraper developer credentials are the repository secrets `SCREENSCRAPER_DEVID` and
`SCREENSCRAPER_DEVPASSWORD`, passed as BuildKit secrets (`SOVEMU_BUILD_SCREENSCRAPER_*`,
`docker-bake.hcl`) and built in by `SovEmu.Server/build.rs`: never build arguments, never in the
tree. NAS credentials live in `.env` (gitignored and dockerignored; see `.env.example`), read by
`compose.nas.yaml`.

## Compose

`compose.yaml` builds the image, mounts `./data` at `/sov-data` and `./data/roms` read-only at
`/sov-library`, and runs the server. In a container the server cannot see the host's address, so
`SovEmu__Streaming__AdvertiseAddress` (default `127.0.0.1` in compose) must be the address browsers
reach the host on. `compose.gpu.yaml`, `compose.nvidia.yaml` and `compose.nas.yaml` add a GPU or
the NAS library.

```bash
docker build --platform linux/amd64 -f SovEmu.Backend/Dockerfile --target test .   # tests
docker compose build && docker compose run --rm sovemu sovemu-worker probe
scripts/fetch-free-roms.sh && docker compose up --build     # then open http://localhost:8080
```

## CI and hooks

CI (`.github/workflows/ci.yml`) lints the backend (`Backend (lint)`: fmt and clippy), runs its
tests once in the container's `test` stage, checks and builds the web client, and runs coverage on
`main` and by hand, and on `main` publishes the image to GHCR as `bleeding` and
`bleeding-<UTC stamp>`. Nothing in CI runs on macOS. The pre-push hook (`.githooks/pre-push`) runs the
same checks for whatever a push changes.

**Each commit's image is built once** (#245). The merge queue builds and tests the commit `main`
will get and pushes it as `sha-<commit>`; the push to `main` retags that image as `bleeding`
(`docker buildx imagetools create`) without compiling. `main` builds only when that image is
missing (a push that skipped the queue) or when the Cargo caches for this `Cargo.lock`, workspace
`Cargo.toml` and Dockerfile are not saved yet (a change to any of them changes how things compile): it then saves them, once per key, with SovEmu's own crates pruned (they
recompile every commit). Pull requests and the queue only read the caches.

## Never commit

Cores, ROMs, BIOS files or data: `cores/`, `roms/`, `system/` and `data/` are gitignored.
`scripts/fetch-cores.sh` downloads cores for local testing, `scripts/fetch-free-roms.sh` freely
licensed homebrew.

## Codecs

**SovEmu builds its own FFmpeg, for H.264 only** (`ffmpeg` stage). Never install Debian's
`libavcodec61` or `libavcodec-dev` in the image or the toolchain: it brings every codec Debian
builds (~90 MB) for three encoders. A new codec or encoder SovEmu.Media opens by name is added to
the stage's `--enable-encoder`/`--enable-decoder` list, or it is missing at run time
(`avcodec_find_encoder_by_name` returns null). Moving FFMPEG_VERSION also moves the About page's
entry (`about.rs`; a test checks they agree).
