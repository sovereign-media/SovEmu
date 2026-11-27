#!/usr/bin/env bash
# Measures what one game session's worker uses on each system (sovereign-media/SovEmu#282): the
# image's `sovemu-worker serve` (VP8 unless CODEC says), started as the server starts a session
# (rewind, run-ahead and prescale at their defaults, the system's core options), played for
# SECONDS by a headless Chromium (client.mjs), sampled by sample.sh.
#
#   scripts/worker-limits/run.sh [OUT] [SECONDS] [CASE...]   # OUT: test-results/worker-limits
#
# Needs Docker, the image (IMAGE, default ghcr.io/sovereign-media/sovemu:bleeding), the free
# ROMs (scripts/fetch-free-roms.sh) and SovEmu.Web's Playwright (npm ci there). Never commit
# the output's logs: they are only numbers, but a core can log a ROM's own text.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
here="$root/scripts/worker-limits"
out="${1:-$root/test-results/worker-limits}"
seconds="${2:-90}"
shift 2 || shift $# || true
image="${IMAGE:-ghcr.io/sovereign-media/sovemu:bleeding}"

# case | system | core | ROM | prescale | run-ahead | core options | presses (frame:buttons). The
# `-interp` cases run the JIT cores' interpreters: under x86 emulation (Docker on Apple silicon)
# their JITs crash, and the interpreters' memory is what a host with a working JIT uses, give or
# take the JIT's code cache.
cases() {
  cat <<'EOF'
nes-nova|nes|fceumm|nes/Nova the Squirrel.nes|2|1||200:start,300:start,420:a,470:down,500:a,600:right,660:right,720:right,780:right,840:right,900:a
nes-240p|nes|fceumm|nes/240p Test Suite.nes|2|1||200:start,400:right,430:down,450:down,470:down,500:a
snes-castle|snes|snes9x|snes/Castle Platformer.sfc|2|1||300:right,400:b,490:b,580:b,670:b,760:b
snes-240p|snes|snes9x|snes/240p Test Suite.sfc|2|1||450:b,490:down,520:down,540:down,560:b
gb-240p|gb|gambatte|gb/240p Test Suite.gb|1|1||200:start,400:right,430:down,450:down,470:down,500:a
gbc-ucity|gbc|gambatte|gbc/uCity.gbc|2|1||200:start,330:down,360:a,450:a,480:down,500:down,520:down,540:down,560:down,590:right,720:a,780:a
gba-240p|gba|mgba|gba/240p Test Suite.gba|2|1||200:start,400:right,430:down,450:down,470:down,500:a
megadrive-240p|megadrive|genesis_plus_gx|megadrive/240p Test Suite.md|2|1||450:down,490:y,530:down,550:down,570:down,590:down,620:y,660:y
pcengine-240p|pcengine|mednafen_pce_fast|pcengine/240p Test Suite.pce|2|1||450:down,490:a,530:down,550:down,570:down,590:down,620:a,660:a
psx-240p|psx|pcsx_rearmed|psx/240p Test Suite.cue|1|1||450:down,470:down,490:down,510:down,530:down,560:b
n64-240p|n64|mupen64plus_next|n64/240p Test Suite.z64|1|0||300:start,450:a,520:down,560:down,600:a
dreamcast-240p|dreamcast|flycast|dreamcast/240p Test Suite.cdi|1|0|reicast_threaded_rendering=disabled reicast_per_content_vmus=All VMUs|600:start,750:a,820:down,860:down,900:a
gamecube-240p|gamecube|dolphin|gamecube/240p Test Suite.iso|1|0||600:start,750:a,820:down,860:down,900:a
psx-240p-interp|psx|pcsx_rearmed|psx/240p Test Suite.cue|1|1|pcsx_rearmed_drc=disabled|450:down,470:down,490:down,510:down,530:down,560:b
n64-240p-interp|n64|mupen64plus_next|n64/240p Test Suite.z64|1|0|mupen64plus-cpucore=cached_interpreter|300:start,450:a,520:down,560:down,600:a
psx-240p-noahead|psx|pcsx_rearmed|psx/240p Test Suite.cue|1|0||450:down,470:down,490:down,510:down,530:down,560:b
gamecube-240p-interp|gamecube|dolphin|gamecube/240p Test Suite.iso|1|0|dolphin_cpu_core=5 dolphin_fastmem=disabled dolphin_fastmem_arena=disabled|600:start,750:a,820:down,860:down,900:a
EOF
}

mkdir -p "$out" && out="$(cd "$out" && pwd)"
while IFS='|' read -r name system core rom prescale ahead options presses; do
  if [ "$#" -gt 0 ] && [[ " $* " != *" $name "* ]]; then continue; fi
  echo "== $name"
  rm -rf "${out:?}/$name" && mkdir -p "$out/$name"
  vars=()
  if [ -n "$options" ]; then
    # Each option is key=value, separated by a space before the next key.
    while read -r kv; do vars+=(--var "$kv"); done < <(sed -E 's/ ([a-z0-9_]+=)/\n\1/g' <<<"$options")
  fi
  docker rm -f sovemu-limits >/dev/null 2>&1 || true
  # /dev/shm as compose.yaml has it: Dolphin maps the GameCube's memory there.
  docker run -d --name sovemu-limits --platform linux/amd64 --shm-size 1g \
    -p 127.0.0.1:8090:8090 -p 127.0.0.1:40000:40000/udp \
    -v "$root/data/roms:/sov-library:ro" -v "$here:/bench:ro" -v "$out/$name:/out" \
    --entrypoint bash "$image" /bench/sample.sh /out -- \
    /opt/sovemu/bin/sovemu-worker serve --core "/opt/sovemu/cores/${core}_libretro.so" \
    --rom "/sov-library/$rom" --http 0.0.0.0:8090 --udp 0.0.0.0:40000 --advertise-ip 127.0.0.1 \
    --web-dir /opt/sovemu/web --system-dir /tmp/system --save-dir /tmp/saves --video-codec "${CODEC:-vp8}" \
    --video-prescale "$prescale" --run-ahead "$ahead" ${vars[@]+"${vars[@]}"} >/dev/null
  for _ in $(seq 1 600); do
    curl -fsS -o /dev/null http://127.0.0.1:8090/ 2>/dev/null && break
    sleep 0.5
  done
  node "$here/client.mjs" http://127.0.0.1:8090 "$seconds" "$presses" >"$out/$name/client.json" \
    2>"$out/$name/client.log" || echo "client failed: $(tail -3 "$out/$name/client.log")"
  # End the session (serve has no shutdown of its own; saves are not measured here).
  docker exec sovemu-limits sh -c \
    'for p in /proc/[0-9]*; do [ "$(cat $p/comm)" = sovemu-worker ] && kill -TERM "${p#/proc/}"; done' \
    2>/dev/null || true
  docker wait sovemu-limits >/dev/null
  docker logs sovemu-limits | tail -1
  cat "$out/$name/client.json"
  docker rm sovemu-limits >/dev/null
done < <(cases)
