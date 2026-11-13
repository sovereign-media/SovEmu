# syntax=docker/dockerfile:1
#
# The SovEmu image: Alpine (musl), self-contained. Build from the repository root:
#
#   docker build -f SovEmu.Backend/Dockerfile -t sovemu .
#   docker build -f SovEmu.Backend/Dockerfile --target test .     # run the test suite on musl
#
# Two things make Alpine work for a libretro frontend:
#
# - The libretro buildbot's Linux cores are glibc builds and will not load on musl, so the
#   `cores` stage builds every core in scripts/cores.list from source, at a pinned commit.
# - A statically linked musl binary cannot dlopen anything, and Rust's musl target links
#   statically by default. RUSTFLAGS turns that off: the worker links musl dynamically.
#
# The final stage runs `sovemu-worker probe` over every core it ships, so a core that cannot be
# loaded fails the image build rather than a player's session.

ARG ALPINE_VERSION=3.24
ARG RUST_VERSION=1.100

## ---- cores: libretro cores from source -------------------------------------------------------
# Independent of the Rust sources, so it stays cached until scripts/cores.list changes.
FROM alpine:${ALPINE_VERSION} AS cores
RUN apk add --no-cache bash build-base cmake git linux-headers zlib-dev
WORKDIR /build
COPY scripts/cores.list scripts/build-cores.sh ./
RUN ./build-cores.sh --out /out/cores --work /build/src && rm -rf /build/src

## ---- build: the Rust workspace -----------------------------------------------------------------
FROM rust:${RUST_VERSION}-alpine${ALPINE_VERSION} AS build
# clang-dev: libclang for bindgen. build-base: the C compiler for the log shim.
RUN apk add --no-cache build-base clang-dev
ENV RUSTFLAGS="-C target-feature=-crt-static" \
    CARGO_TERM_COLOR=always
WORKDIR /src
COPY SovEmu.Backend/ ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p sovemu-worker \
    && install -D target/release/sovemu-worker /out/sovemu-worker

## ---- test: the full test suite, on musl (not part of the shipped image) -----------------------
FROM build AS test
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo test --workspace --locked

## ---- runtime ------------------------------------------------------------------------------------
FROM alpine:${ALPINE_VERSION} AS runtime
# libgcc: Rust's unwinder. libstdc++: the C++ cores (snes9x, gambatte). tini: PID 1, so the
# per-session worker processes the server will spawn are reaped.
RUN apk add --no-cache libgcc libstdc++ tini \
    && addgroup -S -g 1000 sovemu \
    && adduser -S -D -H -u 1000 -G sovemu -h /data sovemu \
    && mkdir -p /data/roms /data/system /data/saves \
    && chown -R sovemu:sovemu /data

COPY --from=cores /out/cores/ /opt/sovemu/cores/
COPY --from=build /out/sovemu-worker /usr/local/bin/sovemu-worker

ENV SOVEMU_CORES_DIR=/opt/sovemu/cores

# Every shipped core must load under musl with the libraries installed above.
RUN sovemu-worker probe > /opt/sovemu/cores/manifest.json

USER sovemu
WORKDIR /data
VOLUME ["/data"]
ENTRYPOINT ["/sbin/tini", "--", "sovemu-worker"]
CMD ["--help"]
