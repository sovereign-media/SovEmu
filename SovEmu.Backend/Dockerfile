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
# Also the workspace's `rust-version` (SovEmu.Backend/Cargo.toml): move them together.
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

## ---- ffmpeg: libavcodec and libavutil, H.264 only -------------------------------------------------
# SovEmu.Media uses FFmpeg for H.264 alone: x264 on the CPU, VA-API and NVENC on a GPU, and the H.264
# decoder for the tests. Debian's libavcodec61 brings every codec it was built with and their
# libraries (~90 MB: x265, codec2, SVT-AV1, aom, ...); this build has those three encoders and that
# decoder, ~3 MB (#236). The test stages link the same build, so the tests encode with what ships.
# A pinned release, checked, so it stays cached until FFMPEG_VERSION changes. NVENC needs only
# NVIDIA's headers (nv-codec-headers, MIT): the driver's library is loaded at run time.
FROM debian:${DEBIAN_RELEASE}-slim AS ffmpeg
RUN apt-get update \
    && apt-get install -y --no-install-recommends -o Acquire::Retries=5 -o Acquire::http::Timeout=30 \
        ca-certificates curl xz-utils make gcc libc6-dev nasm pkg-config libx264-dev libva-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
# Also SovEmu.Server's about.rs, the About page (a test there checks they agree).
ARG FFMPEG_VERSION=7.1.5
ARG FFMPEG_SHA256=de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f
ARG NV_CODEC_HEADERS_VERSION=n12.2.72.0
ARG NV_CODEC_HEADERS_SHA256=dbeaec433d93b850714760282f1d0992b1254fc3b5a6cb7d76fc1340a1e47563
RUN curl -sSfL --retry 5 -o nv-codec-headers.tar.gz \
        "https://github.com/FFmpeg/nv-codec-headers/archive/refs/tags/${NV_CODEC_HEADERS_VERSION}.tar.gz" \
    && echo "${NV_CODEC_HEADERS_SHA256}  nv-codec-headers.tar.gz" | sha256sum -c - \
    && tar xzf nv-codec-headers.tar.gz \
    && make -C "nv-codec-headers-${NV_CODEC_HEADERS_VERSION}" PREFIX=/usr/local install \
    && curl -sSfL --retry 5 -o ffmpeg.tar.xz "https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz" \
    && echo "${FFMPEG_SHA256}  ffmpeg.tar.xz" | sha256sum -c - \
    && tar xJf ffmpeg.tar.xz
# Everything off (no other codec, no formats, filters, scaling or programs, no library found by
# itself), then exactly what SovEmu.Media opens by name. GPL for x264, as Debian's build is.
RUN cd "ffmpeg-${FFMPEG_VERSION}" \
    && ./configure --prefix=/opt/ffmpeg --enable-shared --disable-static \
        --disable-programs --disable-doc --disable-network --disable-autodetect \
        --disable-avformat --disable-avdevice --disable-avfilter --disable-swscale --disable-swresample \
        --disable-everything --enable-gpl --enable-libx264 --enable-vaapi --enable-ffnvcodec --enable-nvenc \
        --enable-encoder=libx264,h264_vaapi,h264_nvenc --enable-decoder=h264 \
    && make -j"$(nproc)" && make install \
    # The runtime takes only the libraries; the licence travels with them (GPL-2.0-or-later).
    && mkdir -p /out/lib /out/doc \
    && cp -a /opt/ffmpeg/lib/libavcodec.so.* /opt/ffmpeg/lib/libavutil.so.* /out/lib/ \
    && cp LICENSE.md COPYING.GPLv2 /out/doc/

## ---- toolchain: Rust and the build dependencies ---------------------------------------------------
# Everything the compiles need but SovEmu's sources, so the dependency stages below (`deps-release`,
# `deps-test`) depend on it and on the cargo-chef recipe only, never on a source file (#245).
FROM rust:${RUST_VERSION}-${DEBIAN_RELEASE} AS toolchain
# libclang-dev: bindgen. cmake: aws-lc (str0m's DTLS). libvpx-dev, libopus-dev: the codecs
# SovEmu.Media links. libx264-164, libva2, libva-drm2: what the `ffmpeg` stage's libavcodec links,
# which SovEmu.Media builds against (pkg-config) and the tests load.
RUN apt-get update \
    && apt-get install -y --no-install-recommends -o Acquire::Retries=5 -o Acquire::http::Timeout=30 clang libclang-dev cmake pkg-config libvpx-dev libopus-dev libx264-164 libva2 libva-drm2 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=ffmpeg /opt/ffmpeg/ /opt/ffmpeg/
RUN echo /opt/ffmpeg/lib > /etc/ld.so.conf.d/sovemu-ffmpeg.conf && ldconfig
ENV PKG_CONFIG_PATH=/opt/ffmpeg/lib/pkgconfig
# No incremental compilation (a one-off build gains nothing from it, and its files would bloat the
# cached layers); tests keep line tables, enough for a backtrace, not full debug info.
ENV CARGO_TERM_COLOR=always \
    CARGO_INCREMENTAL=0 \
    CARGO_PROFILE_DEV_DEBUG=line-tables-only
# cargo-about, which lists the licences of the crates the image is built from (`notices`), and
# cargo-chef, which compiles the dependencies apart from SovEmu's own crates (`deps-*`): their
# released binaries, pinned and checked.
ARG CARGO_ABOUT_VERSION=0.9.3
ARG CARGO_ABOUT_SHA256=3ad1e94ea1e63d69d1961f4fb15ccbbe9575d906111f96d2cb4dce6df48f71b4
ARG CARGO_CHEF_VERSION=0.1.78
ARG CARGO_CHEF_SHA256=aca691abfbfbbe00d482e0ed2249eec3091b65e96a0ce92947fb5b254d48b16d
RUN curl -sSfL --retry 5 -o /tmp/cargo-about.tar.gz \
        "https://github.com/EmbarkStudios/cargo-about/releases/download/${CARGO_ABOUT_VERSION}/cargo-about-${CARGO_ABOUT_VERSION}-x86_64-unknown-linux-musl.tar.gz" \
    && echo "${CARGO_ABOUT_SHA256}  /tmp/cargo-about.tar.gz" | sha256sum -c - \
    && tar xzf /tmp/cargo-about.tar.gz --strip-components=1 -C /usr/local/cargo/bin \
        "cargo-about-${CARGO_ABOUT_VERSION}-x86_64-unknown-linux-musl/cargo-about" \
    && rm /tmp/cargo-about.tar.gz \
    && curl -sSfL --retry 5 -o /tmp/cargo-chef.tar.xz \
        "https://github.com/LukeMathWalker/cargo-chef/releases/download/v${CARGO_CHEF_VERSION}/cargo-chef-x86_64-unknown-linux-musl.tar.xz" \
    && echo "${CARGO_CHEF_SHA256}  /tmp/cargo-chef.tar.xz" | sha256sum -c - \
    && tar xJf /tmp/cargo-chef.tar.xz --strip-components=1 -C /usr/local/cargo/bin \
        cargo-chef-x86_64-unknown-linux-musl/cargo-chef \
    && rm /tmp/cargo-chef.tar.xz
WORKDIR /src

## ---- sources and the recipe ---------------------------------------------------------------------------
FROM toolchain AS sources
COPY SovEmu.Backend/ ./

# The recipe: the workspace's manifests and lockfile, which is all the dependency stages see. It is
# written again for every commit, but its content (and so the stages that copy it) only changes
# with a Cargo.toml or Cargo.lock.
FROM sources AS planner
RUN cargo chef prepare --recipe-path /recipe.json

## ---- notices: the crates' licences, listed again from the lockfile --------------------------------
# cargo-about (`about.toml`'s accepted licences: any other fails the build) writes the list the
# runtime stage puts beside the server binary, which reads it there, so the image always ships the
# current one; the committed copy, built in, is a local build's. Nothing compiles it in, so the
# release build never waits for it.
FROM sources AS notices
RUN --mount=type=cache,id=cargo-registry-notices,target=/usr/local/cargo/registry \
    mkdir -p /out \
    && cargo about generate --locked -c about.toml about.hbs -o /out/third-party-notices.json

## ---- deps-release and build: the release binaries -----------------------------------------------
# The dependencies, compiled from the recipe alone, so this layer (and its registry) stays cached
# until the lockfile or a manifest changes; CI keeps it in BuildKit's GitHub cache (the
# `deps-release` bake target). Then `build` compiles only SovEmu's own crates on top of it.
FROM toolchain AS deps-release
# No debug info: the binaries are stripped of it in `build`, so generating the line tables
# (`debug = "line-tables-only"` in the workspace's release profile, which local release builds
# keep) cost about 7% of the compile for nothing (#245). Symbol names stay for backtraces.
ENV CARGO_PROFILE_RELEASE_DEBUG=0
COPY --from=planner /recipe.json /recipe.json
RUN cargo chef cook --release --locked -p sovemu-worker -p sovemu-server --recipe-path /recipe.json

# One compile for the test suite's wait and the image both: nothing here varies by target or by
# commit (the build stamp is written by the runtime stage), so a bake of both runs it once.
FROM deps-release AS build
COPY SovEmu.Backend/ ./
# SovEmu's own ScreenScraper developer credentials, built into the server (`build.rs`) so a
# released image needs only the admin's ScreenScraper account. BuildKit secrets, never build
# arguments: they reach this one compile as variables and stay out of the image's history and
# layers. CI passes them (docker-bake.hcl); without them (a local build, a fork) none are built
# in, and SovEmu__Metadata__ScreenScraper__DevId/_DEVPASSWORD can still be set at run time.
RUN --mount=type=secret,id=screenscraper_devid,env=SOVEMU_BUILD_SCREENSCRAPER_DEVID \
    --mount=type=secret,id=screenscraper_devpassword,env=SOVEMU_BUILD_SCREENSCRAPER_DEVPASSWORD \
    cargo build --release --locked -p sovemu-worker -p sovemu-server \
    && install -D target/release/sovemu-worker /out/sovemu-worker \
    && install -D target/release/sovemu-server /out/sovemu-server \
    # Drop any debug info that still comes in (the standard library's) but keep symbol names.
    && strip --strip-debug /out/sovemu-worker /out/sovemu-server \
    # Only the binaries leave this stage: without target/ its layer is small, and the image's
    # cache export carries no compiled crates.
    && rm -rf target

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
# and checked, rather than `cargo install`.
ARG NEXTEST_VERSION=0.9.152
ARG NEXTEST_SHA256=d039b43c3e0958441b6ad0a39c8884d018b3d2e8fb9928f209074b1100b5e17a
RUN curl -sSfL --retry 5 -o /tmp/nextest.tar.gz \
        "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-${NEXTEST_VERSION}/cargo-nextest-${NEXTEST_VERSION}-x86_64-unknown-linux-gnu.tar.gz" \
    && echo "${NEXTEST_SHA256}  /tmp/nextest.tar.gz" | sha256sum -c - \
    && tar xzf /tmp/nextest.tar.gz -C /usr/local/cargo/bin \
    && rm /tmp/nextest.tar.gz

# The test build's dependencies, from the recipe alone, as `deps-release` (bake target `deps-test`).
FROM test-base AS deps-test
COPY --from=planner /recipe.json /recipe.json
RUN cargo chef cook --locked --workspace --all-targets --recipe-path /recipe.json

FROM deps-test AS test
COPY SovEmu.Backend/ ./
# Compiled and run beside the release build, which by then is mostly the worker's single-threaded
# optimise-and-link (one codegen unit, thin LTO). Waiting for it put the suite's ~2m 20s after
# that ~4 minutes (#245). The tests that time the emulator compare ratios over several samples
# (fast-forward against normal play), not rates, so a busy runner does not change their answer.
# With the test core, the stand-in core the tests load: SovEmu.TestCore/locate.rs builds it on first
# use, the same way, and every test that loads it waited on that compile while the suite ran
# (half the suite's time: 39 s against 19 s with it built, #245). Built here, the tests find it
# ready and locate.rs's own build has nothing to do.
RUN cargo nextest run --workspace --locked --no-run \
    && cargo build --locked -p sovemu-test-core --features sovemu-libretro-ffi/frontend
RUN cargo nextest run --workspace --locked --profile ci

## ---- coverage: the test suite's line coverage (CI's Coverage job; not part of the image) -------
# The same tests as `test`, instrumented by cargo-llvm-cov, in a target cache of their own. It
# reports, never gates: a test that fails here (an instrumented build is slower, and some tests
# time the emulator) still leaves a report, and `test` is what must pass.
FROM test-base AS coverage-run
ARG CARGO_LLVM_COV_VERSION=0.9.2
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry \
    rustup component add llvm-tools-preview \
    && cargo install cargo-llvm-cov --locked --version "${CARGO_LLVM_COV_VERSION}"
COPY SovEmu.Backend/ ./
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
# encoding. libx264-164: H.264 on the CPU, through the `ffmpeg` stage's libavcodec (copied below,
# with VA-API and NVENC for a GPU). tini: PID 1, so the per-session workers are reaped. libpng, zstd, xz: the 3D cores'
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
        libvpx9 libopus0 libx264-164 libpng16-16t64 libzstd1 liblzma5 \
        libva2 libva-drm2 intel-media-va-driver mesa-va-drivers \
        libegl1 libegl-mesa0 libgl1 libgles2 libgl1-mesa-dri \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -g 1000 sovemu \
    && useradd -u 1000 -g sovemu -d /sov-data -M -s /usr/sbin/nologin sovemu \
    && mkdir -p /sov-data /sov-library \
    && chown sovemu:sovemu /sov-data

COPY --from=cores /out/cores/ /opt/sovemu/cores/
# H.264: the `ffmpeg` stage's libavcodec and libavutil, and their licence.
COPY --from=ffmpeg /out/lib/ /usr/local/lib/
COPY --from=ffmpeg /out/doc/ /usr/share/doc/sovemu-ffmpeg/
RUN ldconfig
COPY --from=build /out/sovemu-worker /out/sovemu-server /opt/sovemu/bin/
# The crates' licences for the admin About page, read beside the server (SovEmu.Server notices.rs).
COPY --from=notices /out/third-party-notices.json /opt/sovemu/bin/third-party-notices.json
COPY --from=web /src/SovEmu.Web/dist/ /opt/sovemu/web/

# The runtime (everything the image ships, read-only) is /opt/sovemu: bin/, cores/, web/ and
# artwork/. The data (everything SovEmu writes) is one volume, /sov-data, whose artwork/ has the
# same layout as /opt/sovemu/artwork: a file there replaces the built-in one at the same path. The
# library is its own read-only mount, /sov-library.
# UTC unless the container is given its own (`-e TZ=Europe/London`, compose's TZ, the chart's
# `timezone`, or a cluster's injector such as k8tz).
ENV TZ=UTC \
    PATH=/opt/sovemu/bin:$PATH \
    SovEmu__Paths__Cores=/opt/sovemu/cores \
    SovEmu__Paths__Web=/opt/sovemu/web \
    SovEmu__Paths__BuiltInArtwork=/opt/sovemu/artwork \
    SovEmu__Paths__Worker=/opt/sovemu/bin/sovemu-worker \
    SovEmu__Paths__Data=/sov-data \
    SovEmu__Paths__Library=/sov-library \
    SovEmu__Streaming__Udp__Ports=50000-50009 \
    # What the NVIDIA container toolkit injects when a GPU is requested: OpenGL/EGL and Vulkan
    # (graphics) and NVENC (video), not only the default compute and utility. Ignored elsewhere.
    NVIDIA_DRIVER_CAPABILITIES=graphics,video,utility

# Every shipped core must load with the libraries installed above.
RUN sovemu-worker probe > /opt/sovemu/cores/manifest.json
# The built-in artwork: backgrounds, system photos, the `sovemu` decorations (drawn by the
# server's own code) and the ESRB's rating icons. The admin's own go at the same paths in
# /sov-data/artwork (and SovEmu__Decorations__Directories, SovEmu__Appearance__BackgroundDirectories).
RUN sovemu-server --write-artwork /opt/sovemu/artwork

# Which build this is, for the admin About page: the commit and its time, which CI passes
# (docker-bake.hcl), written beside the server, which reads it there (SovEmu.Server about.rs).
# Empty in a local build. Last, so a new commit changes only this layer and the ones after it.
ARG SOVEMU_REVISION=""
ARG SOVEMU_BUILT=""
RUN printf '{"revision":"%s","built":"%s"}\n' "$SOVEMU_REVISION" "$SOVEMU_BUILT" > /opt/sovemu/bin/build.json

USER sovemu
WORKDIR /sov-data
VOLUME ["/sov-data"]
# 8080: the web client and API. 50000-50009/udp: one port per concurrent game session.
# 3478 is the built-in TURN relay, always on, over UDP and TCP.
EXPOSE 8080/tcp 50000-50009/udp 3478/udp 3478/tcp
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["sovemu-server"]
