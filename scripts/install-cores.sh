#!/usr/bin/env bash
# Installs the libretro cores listed in scripts/cores.list from a libretro stable release:
# downloads the release's Linux x86_64 core bundle, checks its SHA-256, extracts the listed
# cores, and fetches the data folders some cores need.
#
#   scripts/install-cores.sh --out /opt/sovemu/cores
#
# Used by the container build (Debian, x86_64). Needs bash, curl, 7z (7zip), git and sha256sum.
# For local testing on macOS, scripts/fetch-cores.sh downloads macOS builds instead.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
list="$here/cores.list"
out=""
while [ $# -gt 0 ]; do
  case "$1" in
    --out) out="$2"; shift 2 ;;
    -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[ -n "$out" ] || { echo "--out is required" >&2; exit 2; }
mkdir -p "$out"
out="$(cd "$out" && pwd)"

version="" sha="" cores=() data=()
while read -r kind a b c d; do
  case "$kind" in
    ''|'#'*) ;;
    release) version="$a" sha="$b" ;;
    core) cores+=("$a") ;;
    data) data+=("$a $b $c $d") ;;
    *) echo "cores.list: unknown line '$kind'" >&2; exit 1 ;;
  esac
done < "$list"
[ -n "$version" ] && [ -n "$sha" ] || { echo "cores.list: no release line" >&2; exit 1; }

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

url="https://buildbot.libretro.com/stable/$version/linux/x86_64/RetroArch_cores.7z"
echo "==> libretro $version cores: $url"
curl --fail --silent --show-error --location --retry 5 --retry-all-errors -o "$tmp/cores.7z" "$url"
echo "$sha  $tmp/cores.7z" | sha256sum --check --quiet \
  || { echo "RetroArch_cores.7z does not match the SHA-256 in cores.list" >&2; exit 1; }

listing="$(7z l -ba "$tmp/cores.7z")"
for core in "${cores[@]}"; do
  path="$(printf '%s\n' "$listing" | grep -oE "[^ ]*/${core}_libretro\.so$" | head -n 1)"
  [ -n "$path" ] || { echo "$core is not in libretro $version" >&2; exit 1; }
  7z e -y -bso0 -bsp0 -o"$out" "$tmp/cores.7z" "$path"
  echo "    -> $out/${core}_libretro.so"
done

for entry in "${data[@]}"; do
  read -r repo commit path dest <<< "$entry"
  echo "==> $path @ ${commit:0:12} from $repo"
  src="$tmp/data-$(basename "$repo" .git)"
  git init --quiet "$src"
  git -C "$src" sparse-checkout set --no-cone "/$path/"
  git -C "$src" fetch --quiet --depth 1 --filter=blob:none "$repo" "$commit"
  git -C "$src" checkout --quiet FETCH_HEAD
  mkdir -p "$out/system/$(dirname "$dest")"
  rm -rf "$out/system/$dest"
  cp -r "$src/$path" "$out/system/$dest"
  echo "    -> $out/system/$dest"
done

echo "installed ${#cores[@]} cores from libretro $version"
