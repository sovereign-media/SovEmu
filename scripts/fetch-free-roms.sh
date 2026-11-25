#!/usr/bin/env bash
# Downloads a small library of freely redistributable homebrew into a ROM directory, one folder
# per system using the Recalbox/Batocera names SovEmu understands. Every file is pinned by
# SHA-256 and its licence is listed below; nothing here is a commercial game.
#
#   scripts/fetch-free-roms.sh                 # into data/roms (what compose mounts)
#   scripts/fetch-free-roms.sh /path/to/roms
#
# For trying SovEmu out before a library of your own is connected.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
dest="${1:-$root/data/roms}"
mkdir -p "$dest"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }

# system  file name in the library  sha256  url  [path inside a zip]  licence
while IFS='|' read -r system name sum url inner licence; do
  [ -n "$system" ] || continue
  out="$dest/$system/$name"
  if [ -f "$out" ]; then echo "have  $system/$name"; continue; fi
  mkdir -p "$dest/$system"
  file="$tmp/download"
  curl --fail --silent --show-error --location -o "$file" "$url"
  got="$(sha256 "$file")"
  [ "$got" = "$sum" ] || { echo "checksum mismatch for $url: $got" >&2; exit 1; }
  if [ -n "$inner" ]; then
    rm -rf "$tmp/zip" && mkdir "$tmp/zip" && unzip -q "$file" -d "$tmp/zip"
    cp "$tmp/zip/$inner" "$out"
  else
    cp "$file" "$out"
  fi
  echo "fetched $system/$name  ($licence)"
done <<'EOF'
nes|240p Test Suite.nes|04f01d7372f66ea2befe325b9bd655a9fcc395a31fa46d9466286bb8f9d2e62e|https://github.com/pinobatch/240p-test-mini/releases/download/v0.23/240pee.nes||GPL-2.0, pinobatch/240p-test-mini
nes|Nova the Squirrel.nes|e4780e90b9d1587489bfb797d2ca395be21371ea9262fa9f87f99324ec6960ab|https://github.com/NovaSquirrel/NovaTheSquirrel/releases/download/v1.0.6a/nova.nes||GPL-3.0, NovaSquirrel/NovaTheSquirrel
snes|Castle Platformer.sfc|e75da7af6f3a1bd10c21cf4324d0c4321271ce3b9ef3408b2dc2db9cb05f9159|https://github.com/undisbeliever/castle_platformer/releases/download/v1.04/Castle.Platformer.v1.04.zip|Castle Platformer (v1.04)/Castle Platformer (v1.04).sfc|MIT, undisbeliever/castle_platformer
gb|240p Test Suite.gb|733eb6e7ec0719752ddd5fcda4e715c2f2e20f4ac4bc971c4bd1d49272f9bb04|https://github.com/pinobatch/240p-test-mini/releases/download/v0.23/gb240p.gb||GPL-2.0, pinobatch/240p-test-mini
gbc|uCity.gbc|9422ee2ca7b7ea1d46b58b2a429fff3f354dfd3e732dee1e7ae6220f148ce6e0|https://github.com/AntonioND/ucity/releases/download/v1.3/ucity.gbc||GPL-3.0, AntonioND/ucity
gba|240p Test Suite.gba|47844f7140738a06f8f3bc09780da3ab095539a250b870f614feed561d9d6f34|https://github.com/pinobatch/240p-test-mini/releases/download/v0.23/240pee_mb.gba||GPL-2.0, pinobatch/240p-test-mini
EOF
