#!/usr/bin/env bash
# The two runs behind encode-speed-2026-11.md, from the repository root, with the clips
# captured by scripts/encode-quality-captures.sh into test-results/encode-quality/captures.
set -euo pipefail
cd "$(dirname "$0")/../../../SovEmu.Backend"
cargo build --release -p sovemu-media --example encode_quality
B=target/release/examples/encode_quality
OUT=../test-results/encode-quality/test3
run() {
  local common=(--captures ../test-results/encode-quality/captures --out "$OUT" --codecs vp8
    --kbps 800,1500,2500,5000 --timing-runs 2 --floors no --csv results.csv "$@")
  "$B" bench "${common[@]}" --prescale 2 \
    --clips megadrive-240p-scroll,pcengine-240p-scroll,snes-castle,nes-nova,gba-240p-hill
  "$B" bench "${common[@]}" --prescale 1 --clips psx-240p-scroll
}
# 1: the adaptive speeds and the loop filter's sharpness.
run --cpu-used 8,6,4,2 --sharpness 0,4,7
# 2: fixed speeds, and the fastest adaptive one.
run --cpu-used -1,-2,-4,-8,16 --sharpness 0
