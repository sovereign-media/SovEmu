#!/usr/bin/env python3
"""Downloads the system photos from Wikimedia Commons and turns each into a tile.

    python3 scripts/fetch-system-photos.py            # every photo in PHOTOS not yet in assets/systems
    python3 scripts/fetch-system-photos.py lynx ngp   # just these systems (overwrites)

Each tile is a cut-out on transparency, trimmed, 640 px on its longer side, lossy WebP with alpha,
written to SovEmu.Backend/SovEmu.Server/assets/systems/<system id>.webp. A photo that already has
transparency keeps it; one on a white studio background (Evan-Amos's JPEGs) has the white flood-
filled away from its edges, so white parts of the console itself stay.

Only public-domain and CC0 files are accepted (assets/README.md): the licence is read from
Commons and anything else is refused. After a run, list each new file in BUILTIN_PHOTOS
(SovEmu.Server/src/appearance.rs) and in assets/README.md's table, with the source this prints.

Needs Pillow (`pip install pillow`). Downloads 1280 px thumbnails, one at a time, as Wikimedia
asks of automated clients (https://w.wiki/GHai), and backs off when told to slow down.
"""

import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import deque
from io import BytesIO
from pathlib import Path

from PIL import Image, ImageFilter

# System id → Commons file. Public domain or CC0 only.
PHOTOS = {
    "segacd": "Sega-CD-Model2-Set.png",
    "sg1000": "Sega-SG-1000-Console-Set.jpg",
    "sega32x": "Sega-Genesis-Model2-32X.png",
    "pcengine": "PC-Engine-Console-Set.png",
    "pcenginecd": "NEC-TurboGrafx-16-CD-FL.png",
    "atari2600": "Atari-2600-Wood-4Sw-Set.png",
    "atari7800": "Atari-7800-Console-FL.jpg",
    "lynx": "Atari-Lynx-Handheld-Angled.jpg",
    "ngp": "Neo-Geo-Pocket-Anthra-Left.jpg",
    "ngpc": "Neo-Geo-Pocket-Color-Blue-Left.jpg",
    "wswan": "WonderSwan-Black-Left.jpg",
    "wswanc": "WonderSwan-Color-Blue-Left.jpg",
    "virtualboy": "Virtual-Boy-Set.png",
    "intellivision": "Mattel-Intellivision-Console-FL.jpg",
    "colecovision": "ColecoVision-wController-L.png",
    "vectrex": "GCE-Vectrex-Bare-FL.jpg",
}

ALLOWED_LICENCES = {"Public domain", "CC0"}
API = "https://commons.wikimedia.org/w/api.php"
UA = {"User-Agent": "sovemu-asset-check/1.0 (https://github.com/sovereign-media/SovEmu)"}
THUMB = 1280
LONG_SIDE = 640
WHITE = 236  # a pixel this light on every channel counts as background
OUT = Path(__file__).resolve().parent.parent / "SovEmu.Backend/SovEmu.Server/assets/systems"


def fetch(url: str) -> bytes:
    for attempt in range(8):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=120) as r:
                return r.read()
        except urllib.error.HTTPError as e:
            if e.code != 429:
                raise
            wait = 15 * (attempt + 1)
            print(f"  rate limited, waiting {wait}s", file=sys.stderr)
            time.sleep(wait)
    raise SystemExit(f"still rate limited: {url}")


def describe(files: list[str]) -> dict[str, dict]:
    """Each file's licence, author and original URL, from the Commons API."""
    query = urllib.parse.urlencode(
        {
            "action": "query",
            "format": "json",
            "titles": "|".join(f"File:{f}" for f in files),
            "prop": "imageinfo",
            "iiprop": "extmetadata|url",
        }
    )
    pages = json.loads(fetch(f"{API}?{query}"))["query"]["pages"].values()
    out = {}
    for page in pages:
        name = page["title"].removeprefix("File:").replace(" ", "_")
        if "imageinfo" not in page:
            raise SystemExit(f"{name} is not on Commons")
        info = page["imageinfo"][0]
        meta = info["extmetadata"]
        out[name] = {
            "licence": meta.get("LicenseShortName", {}).get("value", "?"),
            "author": re.sub("<[^>]+>", "", meta.get("Artist", {}).get("value", "?")).strip(),
            "url": info["url"].split("?")[0],
        }
    return out


def thumb_url(original: str) -> str:
    head, name = original.rsplit("/", 1)
    return f"{head.replace('/commons/', '/commons/thumb/', 1)}/{name}/{THUMB}px-{name}"


def cut_out(im: Image.Image) -> Image.Image:
    im = im.convert("RGBA")
    if im.getchannel("A").getextrema()[0] < 250:
        return im  # already a cut-out
    w, h = im.size
    px = im.load()
    background = bytearray(w * h)
    queue = deque()
    for x in range(w):
        queue.extend([(x, 0), (x, h - 1)])
    for y in range(h):
        queue.extend([(0, y), (w - 1, y)])
    while queue:
        x, y = queue.popleft()
        i = y * w + x
        if background[i]:
            continue
        r, g, b, _ = px[x, y]
        if r < WHITE or g < WHITE or b < WHITE:
            continue
        background[i] = 1
        if x > 0:
            queue.append((x - 1, y))
        if x < w - 1:
            queue.append((x + 1, y))
        if y > 0:
            queue.append((x, y - 1))
        if y < h - 1:
            queue.append((x, y + 1))
    mask = Image.frombytes("L", (w, h), bytes(0 if b else 255 for b in background))
    # Soften the edge by a pixel so the cut-out is not jagged once scaled down.
    mask = mask.filter(ImageFilter.MinFilter(3)).filter(ImageFilter.GaussianBlur(1))
    im.putalpha(mask)
    return im


def tile(data: bytes, dest: Path) -> tuple[int, int]:
    im = cut_out(Image.open(BytesIO(data)))
    box = im.getchannel("A").point(lambda a: 255 if a > 8 else 0).getbbox()
    if box:
        im = im.crop(box)
    im.thumbnail((LONG_SIDE, LONG_SIDE), Image.LANCZOS)
    im.save(dest, "WEBP", quality=80, method=6, alpha_quality=90)
    return im.size


def main(only: list[str]) -> None:
    unknown = [s for s in only if s not in PHOTOS]
    if unknown:
        raise SystemExit(f"no photo listed for {', '.join(unknown)}")
    wanted = only or [s for s in PHOTOS if not (OUT / f"{s}.webp").exists()]
    if not wanted:
        print("every listed photo is already a tile")
        return
    info = describe([PHOTOS[s] for s in wanted])
    for system in wanted:
        file = PHOTOS[system].replace(" ", "_")
        meta = info[file]
        if meta["licence"] not in ALLOWED_LICENCES:
            raise SystemExit(f"{file} is {meta['licence']}: only public domain or CC0")
        size = tile(fetch(thumb_url(meta["url"])), OUT / f"{system}.webp")
        print(f"{system}.webp {size[0]}x{size[1]}  ←  {file} | {meta['licence']} | {meta['author']}")
        time.sleep(3)


if __name__ == "__main__":
    main(sys.argv[1:])
