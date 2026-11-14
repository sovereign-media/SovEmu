#!/usr/bin/env bash
# Downloads prebuilt libretro cores from the libretro buildbot (nightly) into cores/ for local
# testing. cores/ is gitignored: cores are never committed.
#
#   scripts/fetch-cores.sh                 # the default set
#   scripts/fetch-cores.sh fceumm mgba     # just these
#
# Platforms: macOS arm64/x86_64, Linux x86_64/aarch64.
set -euo pipefail

DEFAULT_CORES=(fceumm snes9x mgba gambatte genesis_plus_gx)
BUILDBOT="https://buildbot.libretro.com/nightly"

root="$(cd "$(dirname "$0")/.." && pwd)"
dest="$root/cores"
mkdir -p "$dest"

case "$(uname -s)/$(uname -m)" in
  Darwin/arm64)  platform="apple/osx/arm64";  ext="dylib" ;;
  Darwin/x86_64) platform="apple/osx/x86_64"; ext="dylib" ;;
  Linux/x86_64)  platform="linux/x86_64";     ext="so" ;;
  Linux/aarch64) platform="linux/armv8";      ext="so" ;;
  *) echo "unsupported platform: $(uname -s)/$(uname -m)" >&2; exit 1 ;;
esac

cores=("$@")
[ ${#cores[@]} -eq 0 ] && cores=("${DEFAULT_CORES[@]}")

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

for core in "${cores[@]}"; do
  file="${core}_libretro.${ext}"
  url="$BUILDBOT/$platform/latest/$file.zip"
  echo "fetching $core from $url"
  curl --fail --silent --show-error --location -o "$tmp/$file.zip" "$url"
  unzip -o -q "$tmp/$file.zip" -d "$tmp/$core"
  mv -f "$tmp/$core/$file" "$dest/$file"
  echo "  -> cores/$file"
done
