#!/usr/bin/env bash
# Builds the libretro cores listed in scripts/cores.list from source, at their pinned commits.
#
#   scripts/build-cores.sh --out /opt/sovemu/cores            # every core
#   scripts/build-cores.sh --out cores fceumm snes9x          # just these
#
# Used by the container build (Linux, musl or glibc). Needs git, make, a C/C++ toolchain and
# cmake. On macOS for local testing, scripts/fetch-cores.sh is quicker.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
list="$here/cores.list"
out=""
work="${TMPDIR:-/tmp}/sovemu-cores-src"
wanted=()

while [ $# -gt 0 ]; do
  case "$1" in
    --out) out="$2"; shift 2 ;;
    --work) work="$2"; shift 2 ;;
    -h|--help) sed -n '2,8p' "$0"; exit 0 ;;
    *) wanted+=("$1"); shift ;;
  esac
done
[ -n "$out" ] || { echo "--out is required" >&2; exit 2; }
mkdir -p "$out" "$work"
out="$(cd "$out" && pwd)"

jobs="$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 2)"
export MAKEFLAGS="-j$jobs"

built=0
while read -r name repo commit build; do
  case "$name" in ''|'#'*) continue ;; esac
  if [ ${#wanted[@]} -gt 0 ] && [[ ! " ${wanted[*]} " == *" $name "* ]]; then
    continue
  fi

  echo "==> $name @ ${commit:0:12}"
  src="$work/$name"
  rm -rf "$src"
  mkdir -p "$src"
  git -C "$src" init --quiet
  git -C "$src" fetch --quiet --depth 1 "$repo" "$commit"
  git -C "$src" checkout --quiet FETCH_HEAD
  # Some cores carry nested submodules they never build (Mupen64Plus's paraLLEl RSP lists one
  # with no URL); a build that needs a missing one fails on its own.
  git -C "$src" submodule update --quiet --init --recursive --depth 1 \
    || echo "    (some submodules of $name could not be fetched; building anyway)"

  (cd "$src" && eval "$build")

  lib="$(find "$src" -name "${name}_libretro.so" -type f | head -n 1)"
  [ -n "$lib" ] || { echo "build of $name produced no ${name}_libretro.so" >&2; exit 1; }
  strip --strip-unneeded "$lib" 2>/dev/null || true
  cp "$lib" "$out/"
  echo "    -> $out/${name}_libretro.so"
  built=$((built + 1))
done < "$list"

[ "$built" -gt 0 ] || { echo "no cores matched: ${wanted[*]}" >&2; exit 1; }
echo "built $built core(s)"
