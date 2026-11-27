#!/usr/bin/env python3
"""Tables and bitrate curves from encode_quality's CSV (sovereign-media/SovEmu#263).

    python3 -I scripts/encode-quality-report.py results.csv OUT_DIR [--group prescale|screen_content]

Writes OUT_DIR/curves-<system>.svg (ΔE2000 mean against actual bitrate, one line per codec and
variant) and prints Markdown tables per system to stdout.
"""
import csv
import sys
from collections import defaultdict
from pathlib import Path

COLOURS = ["#2a6fdb", "#d9480f", "#2b8a3e", "#ae3ec9", "#e67700", "#1098ad"]
DASH = {"1": "", "2": "6 4"}


def load(path):
    with open(path) as f:
        return list(csv.DictReader(f))


def curves_svg(system, lines, path, x_key="actual_kbps", y_key="de_mean", y_label="ΔE2000 mean"):
    w, h, ml, mr, mt, mb = 560, 340, 60, 170, 30, 46
    pts = [(float(r[x_key]), float(r[y_key])) for rows in lines.values() for r in rows]
    if not pts:
        return
    xmax = max(p[0] for p in pts) * 1.05 or 1
    ymax = max(p[1] for p in pts) * 1.1 or 1
    sx = lambda x: ml + x / xmax * (w - ml - mr)
    sy = lambda y: mt + (1 - y / ymax) * (h - mt - mb)
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" font-family="sans-serif" font-size="11">',
           f'<rect width="{w}" height="{h}" fill="white"/>',
           f'<text x="{ml}" y="18" font-size="13" font-weight="bold">{system}: {y_label} against bitrate (lower is better)</text>']
    for i in range(6):
        y = ymax * i / 5
        out.append(f'<line x1="{ml}" x2="{w-mr}" y1="{sy(y):.1f}" y2="{sy(y):.1f}" stroke="#eee"/>')
        out.append(f'<text x="{ml-6}" y="{sy(y)+4:.1f}" text-anchor="end" fill="#555">{y:.2f}</text>')
    for i in range(6):
        x = xmax * i / 5
        out.append(f'<text x="{sx(x):.1f}" y="{h-mb+16}" text-anchor="middle" fill="#555">{x:.0f}</text>')
    out.append(f'<text x="{(ml+w-mr)/2}" y="{h-10}" text-anchor="middle" fill="#333">actual kbit/s</text>')
    out.append(f'<line x1="{ml}" x2="{w-mr}" y1="{sy(0)}" y2="{sy(0)}" stroke="#999"/>')
    for n, (label, rows) in enumerate(sorted(lines.items())):
        rows = sorted(rows, key=lambda r: float(r[x_key]))
        colour = COLOURS[n % len(COLOURS)]
        d = " ".join(f"{'M' if i == 0 else 'L'}{sx(float(r[x_key])):.1f},{sy(float(r[y_key])):.1f}" for i, r in enumerate(rows))
        out.append(f'<path d="{d}" fill="none" stroke="{colour}" stroke-width="2"/>')
        for r in rows:
            out.append(f'<circle cx="{sx(float(r[x_key])):.1f}" cy="{sy(float(r[y_key])):.1f}" r="3" fill="{colour}"/>')
        ly = mt + 14 + n * 16
        out.append(f'<line x1="{w-mr+12}" x2="{w-mr+34}" y1="{ly-4}" y2="{ly-4}" stroke="{colour}" stroke-width="2"/>')
        out.append(f'<text x="{w-mr+40}" y="{ly}">{label}</text>')
    out.append("</svg>")
    Path(path).write_text("\n".join(out))


def main():
    rows = load(sys.argv[1])
    out = Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    group = sys.argv[sys.argv.index("--group") + 1] if "--group" in sys.argv else "prescale"
    by_system = defaultdict(list)
    for r in rows:
        by_system[r["system"]].append(r)
    for system, srows in sorted(by_system.items()):
        lines = defaultdict(list)
        for r in srows:
            if r["codec"] == "none":
                continue
            # One line per codec and variant, mean over the system's clips at each target rate.
            lines[(r["codec"], r[group], r["target_kbps"])].append(r)
        merged = defaultdict(list)
        for (codec, g, _), rs in lines.items():
            label = f"{codec} {g}x" if group == "prescale" else f"{codec} sc{g}"
            avg = {k: str(sum(float(x[k]) for x in rs) / len(rs)) for k in ("actual_kbps", "de_mean", "de_over_2_3_pct")}
            merged[label].append(avg)
        curves_svg(system, merged, out / f"curves-{system}.svg")
        curves_svg(system, merged, out / f"curves-{system}-visible.svg", y_key="de_over_2_3_pct",
                   y_label="% pixels ΔE>2.3")

        print(f"\n### {system}\n")
        cols = ["clip", "codec", group, "target_kbps", "actual_kbps", "de_mean", "de_over_2_3_pct",
                "de_p99", "worst1pct_de", "psnr_y", "psnr_cb", "psnr_cr", "ssim_y",
                "enc_ms_mean", "enc_ms_p95", "dec_ms_p95", "dropped"]
        print("| " + " | ".join(cols) + " |")
        print("|" + "---|" * len(cols))
        for r in sorted(srows, key=lambda r: (r["clip"], r["codec"], r[group], int(r["target_kbps"]))):
            vals = []
            for c in cols:
                v = r[c]
                try:
                    f = float(v)
                    v = f"{f:.0f}" if c in ("target_kbps", "actual_kbps", "dropped") else f"{f:.4f}" if c == "ssim_y" else f"{f:.2f}"
                except ValueError:
                    pass
                vals.append(v)
            print("| " + " | ".join(vals) + " |")


if __name__ == "__main__":
    main()
