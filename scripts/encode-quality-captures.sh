#!/usr/bin/env bash
# Captures the reference clips for encoder experiments (sovereign-media/SovEmu#263): each clip
# is the core's own frames, every one as a lossless RGBA PNG, before any colour conversion or
# encoding. Every encode is scored against these, so captures are made once and reused.
#
#   scripts/encode-quality-captures.sh [OUT] [CLIP...]   # OUT defaults to test-results/encode-quality/captures
#
# Needs the release worker (cargo build --release -p sovemu-worker), the cores
# (scripts/fetch-cores.sh fceumm snes9x gambatte mgba genesis_plus_gx mednafen_pce_fast pcsx_rearmed) and the free ROMs
# (scripts/fetch-free-roms.sh). Never commit a capture: the frames are the games' pictures.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/test-results/encode-quality/captures}"
shift || true
worker="$root/SovEmu.Backend/target/release/sovemu-worker"
roms="$root/data/roms"
ext="so"; [ "$(uname)" = "Darwin" ] && ext="dylib"

# clip | core | ROM | lead-in frames | worker arguments. A clip is the 1,800 frames (30 s) after
# its lead-in, renumbered from frame_000000; the lead-in (menus, a slow screen) is dropped.
clips() {
  cat <<'EOF'
nes-nova|fceumm|nes/Nova the Squirrel.nes|0|--hold 200-205:start --hold 300-305:start --hold 420-425:a --hold 470-474:down --hold 500-505:a --hold 600-1790:right --hold 700-716:a --hold 900-916:a --hold 1100-1116:a --hold 1300-1316:a --hold 1500-1516:a
nes-240p-hill|fceumm|nes/240p Test Suite.nes|0|--hold 200-205:start --hold 400-404:right --hold 430-434:down --hold 450-454:down --hold 470-474:down --hold 500-504:a
nes-240p-colour|fceumm|nes/240p Test Suite.nes|0|--hold 200-205:start --hold 400-404:down --hold 420-424:down --hold 450-454:a --hold 1050-1054:b --hold 1100-1104:down --hold 1120-1124:down --hold 1150-1154:a
snes-castle|snes9x|snes/Castle Platformer.sfc|0|--hold 300-1790:right --hold 400-414:b --hold 490-504:b --hold 580-594:b --hold 670-684:b --hold 760-774:b --hold 850-864:b --hold 940-954:b --hold 1030-1044:b --hold 1120-1134:b --hold 1210-1224:b --hold 1300-1314:b --hold 1390-1404:b --hold 1480-1494:b --hold 1570-1584:b --hold 1660-1674:b --hold 1750-1764:b
gb-240p-hill|gambatte|gb/240p Test Suite.gb|0|--var gambatte_gb_hwmode=GB --hold 200-205:start --hold 400-404:right --hold 430-434:down --hold 450-454:down --hold 470-474:down --hold 500-504:a
gb-240p-sharpness|gambatte|gb/240p Test Suite.gb|0|--var gambatte_gb_hwmode=GB --hold 200-205:start --hold 400-404:down --hold 420-424:down --hold 440-444:down --hold 460-464:down --hold 500-504:a --hold 1050-1054:b --hold 1100-1104:down --hold 1120-1124:down --hold 1140-1144:down --hold 1160-1164:down --hold 1200-1204:a
gbc-ucity|gambatte|gbc/uCity.gbc|1200|--hold 200-205:start --hold 330-334:down --hold 360-364:a --hold 450-454:a --hold 480-484:down --hold 500-504:down --hold 520-524:down --hold 540-544:down --hold 560-564:down --hold 590-594:right --hold 610-614:right --hold 630-634:right --hold 650-654:right --hold 670-674:right --hold 720-724:a --hold 780-784:a --hold 1350-1354:start --hold 1500-1900:right --hold 1900-2300:down --hold 2300-2700:left --hold 2700-2990:up
gbc-240p-colour|gambatte|gb/240p Test Suite.gb|0|--var gambatte_gb_hwmode=GBC --hold 200-205:start --hold 400-404:a --hold 430-434:down --hold 450-454:down --hold 480-484:a --hold 1050-1054:b --hold 1080-1084:down --hold 1100-1104:down --hold 1130-1134:a
gba-240p-hill|mgba|gba/240p Test Suite.gba|0|--hold 200-205:start --hold 400-404:right --hold 430-434:down --hold 450-454:down --hold 470-474:down --hold 500-504:a
gba-240p-colour|mgba|gba/240p Test Suite.gba|0|--hold 200-205:start --hold 400-404:down --hold 420-424:down --hold 450-454:a --hold 1050-1054:b --hold 1100-1104:down --hold 1120-1124:down --hold 1150-1154:a
megadrive-240p-colour|genesis_plus_gx|megadrive/240p Test Suite.md|400|--hold 450-454:y --hold 490-494:y --hold 530-534:down --hold 560-564:y --hold 1300-1304:start --hold 1340-1344:down --hold 1360-1364:down --hold 1380-1384:down --hold 1400-1404:down --hold 1430-1434:y --hold 1470-1474:y
megadrive-240p-scroll|genesis_plus_gx|megadrive/240p Test Suite.md|400|--hold 450-454:down --hold 490-494:y --hold 530-534:down --hold 550-554:down --hold 570-574:down --hold 590-594:down --hold 620-624:y --hold 660-664:y
pcengine-240p-colour|mednafen_pce_fast|pcengine/240p Test Suite.pce|400|--hold 450-454:a --hold 490-494:down --hold 520-524:a --hold 1300-1304:b --hold 1340-1344:down --hold 1360-1364:down --hold 1380-1384:down --hold 1400-1404:down --hold 1430-1434:a --hold 1470-1474:a
pcengine-240p-scroll|mednafen_pce_fast|pcengine/240p Test Suite.pce|400|--hold 450-454:down --hold 490-494:a --hold 530-534:down --hold 550-554:down --hold 570-574:down --hold 590-594:down --hold 620-624:a --hold 660-664:a
psx-240p-colour|pcsx_rearmed|psx/240p Test Suite.cue|400|--hold 450-454:b --hold 490-494:down --hold 520-524:b --hold 1300-1304:x --hold 1340-1344:down --hold 1360-1364:down --hold 1380-1384:down --hold 1430-1434:b
psx-240p-scroll|pcsx_rearmed|psx/240p Test Suite.cue|400|--hold 450-454:down --hold 470-474:down --hold 490-494:down --hold 510-514:down --hold 530-534:down --hold 560-564:b
EOF
}

mkdir -p "$out"
while IFS='|' read -r clip core rom lead args; do
  if [ "$#" -gt 0 ] && [[ " $* " != *" $clip "* ]]; then continue; fi
  rm -rf "${out:?}/$clip"
  # shellcheck disable=SC2086 # the arguments are split on purpose
  cmd=("$worker" headless --core "$root/cores/${core}_libretro.$ext" --rom "$roms/$rom"
    --frames $((lead + 1800)) --png-every 1 --system-dir "$root/data/bios" --out "$out/$clip" $args)
  "${cmd[@]}" >/dev/null 2>"$out/$clip.log"
  for ((n = 0; n < 1800; n++)); do
    if [ "$lead" -gt 0 ]; then
      mv "$(printf '%s/frame_%06d.png' "$out/$clip" $((n + lead)))" "$(printf '%s/frame_%06d.png' "$out/$clip" "$n")"
    fi
  done
  find "$out/$clip" -name 'frame_*.png' | sort | tail -n +1801 | xargs rm -f
  rm -f "$out/$clip/last.png"
  printf 'sovemu-worker headless --core cores/%s_libretro.%s --rom "data/roms/%s" --frames %s --png-every 1 %s\n# kept frames %s-%s as frame_000000-frame_001799\n' \
    "$core" "$ext" "$rom" $((lead + 1800)) "$args" "$lead" $((lead + 1799)) >"$out/$clip/command.txt"
  printf '%-20s %s frames\n' "$clip" "$(ls "$out/$clip" | grep -c '^frame_')"
done < <(clips)
