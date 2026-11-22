# syntax=docker/dockerfile:1
#
# The SovEmu image: Debian (glibc), x86_64, self-contained. Build from the repository root:
#
#   docker build --platform linux/amd64 -f SovEmu.Backend/Dockerfile -t sovemu .
#   docker build --platform linux/amd64 -f SovEmu.Backend/Dockerfile --target test .   # tests
#
# x86_64 only: the cores come from a libretro stable release, which has no Linux arm64 cores.
# On an arm64 machine (a Mac) Docker builds and runs it under x86_64 emulation.
#
# The container runs `sovemu-server` (web client + API on 8080), which starts one
# `sovemu-worker session` per game, each streaming WebRTC on its own UDP port (50000-50009).
#
# Base: debian:trixie-slim, the smallest Debian that still has apt and a shell (distroless would
# save ~55 MB but means copying Mesa in file by file). Packages are --no-install-recommends.
#
# Why Debian: GPU rendering has to work with whatever driver the host runs. Mesa (in the image)
# covers AMD, Intel and software rendering; NVIDIA's own driver is injected at run time by the
# NVIDIA container toolkit, and its libraries are glibc builds. With glibc and libglvnd here,
# the worker's EGL loads whichever vendor is present, and the same image serves every host.
#
# Cores come prebuilt from a libretro stable release, pinned by version and SHA-256 in
# scripts/cores.list (`cores` stage). The final stage runs `sovemu-worker probe` over every
# core, so a core that cannot be loaded fails the image build rather than a player's session.
#
# The web client (SovEmu.Web, Vite + React) is built in the `web` stage and served as static
# files by the same Rust process that serves the API.

ARG DEBIAN_RELEASE=trixie
ARG RUST_VERSION=1.100
ARG NODE_VERSION=24

## ---- web: the web client (Vite) -------------------------------------------------------------
FROM node:${NODE_VERSION}-${DEBIAN_RELEASE}-slim AS web
# Laid out as in the repository: the build checks every GraphQL document against the server's
# schema (gql.tada), which it reads at ../SovEmu.Backend/SovEmu.Server/schema.graphql.
WORKDIR /src/SovEmu.Web
COPY SovEmu.Web/package.json SovEmu.Web/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci --no-audit --no-fund
COPY SovEmu.Backend/SovEmu.Server/schema.graphql /src/SovEmu.Backend/SovEmu.Server/schema.graphql
COPY SovEmu.Web/ ./
RUN npm test && npm run build

## ---- cores: libretro cores from a stable release ---------------------------------------------
# Independent of the Rust sources, so it stays cached until scripts/cores.list changes.
FROM debian:${DEBIAN_RELEASE}-slim AS cores
ARG TARGETARCH
RUN [ "${TARGETARCH:-amd64}" = amd64 ] || { \
        echo "SovEmu's cores come from libretro's stable x86_64 release: build with --platform linux/amd64" >&2; \
        exit 1; }
# 7zip: the release bundle. git: the data folders. (The release's cores come stripped.)
RUN apt-get update \
    && apt-get install -y --no-install-recommends -o Acquire::Retries=5 -o Acquire::http::Timeout=30 \
        bash ca-certificates curl git 7zip \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY scripts/cores.list scripts/install-cores.sh ./
RUN ./install-cores.sh --out /out/cores

## ---- toolchain: Rust, the build dependencies and the sources --------------------------------------
# `build` and `test` both start here and neither waits for the other, so BuildKit compiles the
# release binaries and the test suite side by side. Each has its own `target/` cache (cargo locks
# a target directory, so a shared one would make them take turns); CI keeps the caches between
# runs (buildkit-cache-dance in .github/workflows/ci.yml).
FROM rust:${RUST_VERSION}-${DEBIAN_RELEASE} AS toolchain
# libclang-dev: bindgen. cmake: aws-lc (str0m's DTLS). libvpx-dev, libopus-dev, libavcodec-dev,
# libavutil-dev: the codecs SovEmu.Media links (libavcodec brings x264 for the H.264 tests).
RUN apt-get update \
    && apt-get install -y --no-install-recommends -o Acquire::Retries=5 -o Acquire::http::Timeout=30 clang libclang-dev cmake pkg-config libvpx-dev libopus-dev libavcodec-dev libavutil-dev \
    && rm -rf /var/lib/apt/lists/*
# No incremental compilation (a one-off build gains nothing from it, and its files would bloat the
# kept cache); tests keep line tables, enough for a backtrace, not full debug info.
ENV CARGO_TERM_COLOR=always \
    CARGO_INCREMENTAL=0 \
    CARGO_PROFILE_DEV_DEBUG=line-tables-only
WORKDIR /src
COPY SovEmu.Backend/ ./

## ---- build: the release binaries ------------------------------------------------------------------
FROM toolchain AS build
# Which build this is, for the admin About page (`option_env!` in sovemu-server): the commit and
# its time, which CI passes (docker-bake.hcl). Empty in a local build. Declared here, not in
# `toolchain`, so the test stage's compile does not change with every commit.
ARG SOVEMU_REVISION=""
ARG SOVEMU_BUILT=""
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=target-release,target=/src/target \
    cargo build --release --locked -p sovemu-worker -p sovemu-server \
    && install -D target/release/sovemu-worker /out/sovemu-worker \
    && install -D target/release/sovemu-server /out/sovemu-server \
    # Drop the line tables (most of the binaries' size) but keep symbol names for backtraces.
    && strip --strip-debug /out/sovemu-worker /out/sovemu-server

## ---- test: the full test suite (not part of the shipped image) ---------------------------------
FROM toolchain AS test-base
# Files tests check the sources against, at the places they sit relative to the sources in the
# repository: the core list (SovEmu.Library's licence table, `cores.rs`) and the README (every
# setting has a row in its table, SovEmu.Server's `configuration.rs`).
COPY scripts/cores.list /scripts/cores.list
COPY README.md /README.md
# Mesa (EGL through libglvnd, desktop GL and GLES, llvmpipe): the GPU-rendering tests draw with
# the software renderer here, as a server without a GPU would.
RUN apt-get update \
    && apt-get install -y --no-install-recommends -o Acquire::Retries=5 -o Acquire::http::Timeout=30 libegl1 libegl-mesa0 libgl1 libgles2 libgl1-mesa-dri \
    && rm -rf /var/lib/apt/lists/*
# cargo-nextest, the test runner (SovEmu.Backend/.config/nextest.toml): its released binary, pinned
# and checked, rather than `cargo install`, which this stage (after the sources) would redo on
# every commit.
ARG NEXTEST_VERSION=0.9.152
ARG NEXTEST_SHA256=d039b43c3e0958441b6ad0a39c8884d018b3d2e8fb9928f209074b1100b5e17a
RUN curl -sSfL --retry 5 -o /tmp/nextest.tar.gz \
        "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-${NEXTEST_VERSION}/cargo-nextest-${NEXTEST_VERSION}-x86_64-unknown-linux-gnu.tar.gz" \
    && echo "${NEXTEST_SHA256}  /tmp/nextest.tar.gz" | sha256sum -c - \
    && tar xzf /tmp/nextest.tar.gz -C /usr/local/cargo/bin \
    && rm /tmp/nextest.tar.gz

FROM test-base AS test
# Compiled beside the release build, but run after it: some tests time the emulator (fast-forward
# must be several times faster than normal play), and a release link on the same cores would
# slow them. Copying from `build` makes BuildKit wait for it before the tests run.
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=target-test,target=/src/target \
    cargo nextest run --workspace --locked --no-run
COPY --from=build /out/sovemu-server /tmp/release-built
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=target-test,target=/src/target \
    rm /tmp/release-built && cargo nextest run --workspace --locked --profile ci

## ---- coverage: the test suite's line coverage (CI's Coverage job; not part of the image) -------
# The same tests as `test`, instrumented by cargo-llvm-cov, in a target cache of their own. It
# reports, never gates: a test that fails here (an instrumented build is slower, and some tests
# time the emulator) still leaves a report, and `test` is what must pass.
FROM test-base AS coverage-run
ARG CARGO_LLVM_COV_VERSION=0.9.2
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry \
    rustup component add llvm-tools-preview \
    && cargo install cargo-llvm-cov --locked --version "${CARGO_LLVM_COV_VERSION}"
# The stand-in core the tests load (SovEmu.TestCore) is test equipment: left out of the reports.
ENV COVERAGE_IGNORE='SovEmu\.TestCore/'
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=target-coverage,target=/src/target \
    mkdir -p /out \
    && cargo llvm-cov nextest --workspace --locked --ignore-run-fail \
         --ignore-filename-regex "$COVERAGE_IGNORE" --lcov --output-path /out/lcov.info \
    && cargo llvm-cov report --ignore-filename-regex "$COVERAGE_IGNORE" --summary-only \
         > /out/summary.txt \
    && cargo llvm-cov report --ignore-filename-regex "$COVERAGE_IGNORE" --html --output-dir /out

# Only the reports: `docker buildx bake coverage` writes them to ./coverage.
FROM scratch AS coverage
COPY --from=coverage-run /out/ /

## ---- runtime ------------------------------------------------------------------------------------
FROM debian:${DEBIAN_RELEASE}-slim AS runtime
# libstdc++6, libgomp1: the C++ cores (and flycast's OpenMP). libxi6, libusb-1.0-0: linked by
# the release's Dolphin (for input devices SovEmu never gives it). libvpx9, libopus0: VP8 and Opus
# encoding. libavcodec61, libavutil59: H.264 (x264 on the CPU; VA-API and NVENC on a GPU). tini: PID 1, so the per-session workers are reaped. libpng, zstd, xz: the 3D cores'
# texture and disc-image formats. tzdata: the time zones, so `TZ` (America/Chicago) gives log lines
# local times.
# GPU rendering: libglvnd (libegl1, libgl1, libgles2) dispatches to a vendor. Mesa's is here
# (libegl-mesa0, libgl1-mesa-dri: AMD radeonsi, Intel iris/crocus, nouveau, and llvmpipe, the
# software renderer used without a GPU). NVIDIA's is added by the NVIDIA container toolkit when
# the host has one. Mesa's Vulkan drivers (mesa-vulkan-drivers, ~80 MB) are left out until a
# shipped core renders with Vulkan.
# GPU encoding: libva (libva2, libva-drm2) with Intel's driver (intel-media-va-driver, Debian main:
# its low-power encoder, the one SovEmu uses, without the non-free shaders) and Mesa's
# (mesa-va-drivers: AMD). NVENC comes with NVIDIA's driver (`video` in its capabilities).
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends -o Acquire::Retries=5 -o Acquire::http::Timeout=30 ca-certificates tini tzdata libstdc++6 libgomp1 \
        libxi6 libusb-1.0-0 \
        libvpx9 libopus0 libavcodec61 libavutil59 libpng16-16t64 libzstd1 liblzma5 \
        libva2 libva-drm2 intel-media-va-driver mesa-va-drivers \
        libegl1 libegl-mesa0 libgl1 libgles2 libgl1-mesa-dri \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -g 1000 sovemu \
    && useradd -u 1000 -g sovemu -d /data -M -s /usr/sbin/nologin sovemu \
    && mkdir -p /data/roms /data/bios /data/saves \
    && chown -R sovemu:sovemu /data

COPY --from=cores /out/cores/ /opt/sovemu/cores/
COPY --from=build /out/sovemu-worker /out/sovemu-server /usr/local/bin/
COPY --from=web /src/SovEmu.Web/dist/ /opt/sovemu/web/

# UTC unless the container is given its own (`-e TZ=Europe/London`, compose's TZ, the chart's
# `timezone`, or a cluster's injector such as k8tz).
ENV TZ=UTC \
    SOVEMU_CORES_DIR=/opt/sovemu/cores \
    SOVEMU_WEB_DIR=/opt/sovemu/web \
    SOVEMU_DECORATIONS_BUILTIN=/opt/sovemu/decorations \
    SOVEMU_WORKER=/usr/local/bin/sovemu-worker \
    SOVEMU_DATA_DIR=/data \
    SOVEMU_UDP_PORTS=50000-50009 \
    # What the NVIDIA container toolkit injects when a GPU is requested: OpenGL/EGL and Vulkan
    # (graphics) and NVENC (video), not only the default compute and utility. Ignored elsewhere.
    NVIDIA_DRIVER_CAPABILITIES=graphics,video,utility

# Every shipped core must load with the libraries installed above.
RUN sovemu-worker probe > /opt/sovemu/cores/manifest.json
# The built-in decoration set (`sovemu`), drawn by the server's own code: SovEmu ships no
# third-party artwork. Other sets go in /data/decorations or SOVEMU_DECORATIONS_DIRS.
RUN sovemu-server --write-decorations /opt/sovemu/decorations

USER sovemu
WORKDIR /data
VOLUME ["/data"]
# 8080: the web client and API. 50000-50009/udp: one port per concurrent game session.
EXPOSE 8080/tcp 50000-50009/udp
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["sovemu-server"]
