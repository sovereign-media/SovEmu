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

# Downloads an itch.io upload (`itch:<user>/<game>/<upload id>`) as the page's free-download
# button does: itch.io hands out short-lived links, so the checksum, not the URL, pins the file.
itch() {
  local user="${1%%/*}" rest="${1#*/}"
  local game="${rest%%/*}" upload="${rest#*/}" page csrf download jar="$tmp/itch-cookies"
  page="https://$user.itch.io/$game"
  csrf="$(curl -fsS -c "$jar" "$page" | grep -o 'csrf_token" value="[^"]*' | head -1 | cut -d'"' -f3)"
  # "No thanks, just take me to the downloads": a download page whose visit lets the session
  # ask for the file's link.
  download="$(curl -fsS -b "$jar" -c "$jar" -X POST --data-urlencode "csrf_token=$csrf" \
    "$page/download_url" | grep -o '"url":"[^"]*' | cut -d'"' -f4 | sed 's|\\/|/|g')"
  csrf="$(curl -fsS -b "$jar" -c "$jar" "$download" \
    | grep -o 'csrf_token" value="[^"]*' | head -1 | cut -d'"' -f3)"
  curl -fsS -b "$jar" -X POST --data-urlencode "csrf_token=$csrf" \
    "$page/file/$upload?source=game_download" | grep -o '"url":"[^"]*' | cut -d'"' -f4 | sed 's|\\/|/|g'
}

# system  file name in the library  sha256  url  [path inside a zip]  licence
# A disc's path inside the zip is its `.cue` and `.bin`, comma-separated: both are copied, named
# as the library file, and the cue is pointed at the renamed bin.
while IFS='|' read -r system name sum url inner licence; do
  [ -n "$system" ] || continue
  out="$dest/$system/$name"
  if [ -f "$out" ]; then echo "have  $system/$name"; continue; fi
  mkdir -p "$dest/$system"
  file="$tmp/download"
  case "$url" in itch:*) url="$(itch "${url#itch:}")" ;; esac
  curl --fail --silent --show-error --location -o "$file" "$url"
  got="$(sha256 "$file")"
  [ "$got" = "$sum" ] || { echo "checksum mismatch for $name: $got" >&2; exit 1; }
  if [ -n "$inner" ]; then
    rm -rf "$tmp/zip" && mkdir "$tmp/zip" && unzip -q "$file" -d "$tmp/zip"
    case "$inner" in
      *.cue,*.bin)
        bin="${out%.*}.bin"
        cp "$tmp/zip/${inner#*,}" "$bin"
        sed "s|^FILE \"[^\"]*\"|FILE \"$(basename "$bin")\"|" "$tmp/zip/${inner%%,*}" > "$out"
        ;;
      *) cp "$tmp/zip/$inner" "$out" ;;
    esac
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
megadrive|240p Test Suite.md|0db29b9739cc75c44dcbf2f3fa74608fff57cf05835e88cf28ffeb1faa4b7970|itch:artemiourbina/240p-test-suite/17035733|240pSuite-1.32.bin|GPL-2.0, ArtemioUrbina/240pTestSuite
pcengine|240p Test Suite.pce|b772532fe00a03e338977b5ab539eff8ecb71b2fb084c771335104d537da08e7|itch:artemiourbina/240p-test-suite/9964366|240pSuite.pce|GPL-2.0, ArtemioUrbina/240pTestSuite
psx|240p Test Suite.cue|63f64dcd61bd35d277f9164d17563afc2de8e1adcd316a8e0a45ea753bfbf542|https://github.com/filipalac/240pTestSuite-PS1/releases/download/19122020/240pTestSuitePS1-EMU.zip|240pTestSuitePS1-EMU.cue,240pTestSuitePS1-EMU.bin|GPL-2.0, filipalac/240pTestSuite-PS1
snes|240p Test Suite.sfc|880696689fa9bfdf395cc47dd4be8a312192f63594f249382945e31f00db7b0c|itch:artemiourbina/240p-test-suite/19403154|240pSuite.sfc|GPL-2.0, ArtemioUrbina/240pTestSuite
n64|240p Test Suite.z64|5b10602187a2c65f87cacbab7f2e70bde1240ee75792eb60ac3eaf8fe21665c0|itch:artemiourbina/240p-test-suite/17334416|240pSuite.z64|GPL-2.0, ArtemioUrbina/240pTestSuite
dreamcast|240p Test Suite.cdi|630cd8aed3f0481ccc4c0351f9bddf53cb99ff360820c60427be3ebd30bde9f7|itch:artemiourbina/240p-test-suite/6581879|240pSuite.cdi|GPL-2.0, ArtemioUrbina/240pTestSuite
gamecube|240p Test Suite.iso|c263a18be2250bebb839e9ca1fcd3e457c0fdd7144968f1f485c8d090d90eb38|itch:artemiourbina/240p-test-suite/17035732|GameCube-240pSuite-1.20.iso|GPL-2.0, ArtemioUrbina/240pTestSuite
EOF
