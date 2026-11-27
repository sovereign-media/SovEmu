#!/usr/bin/env python3
"""The worker-limits bench's table (sovereign-media/SovEmu#282), from run.sh's output.

    python3 -I scripts/worker-limits/report.py [OUT]   # OUT: test-results/worker-limits

One row per case: peak resident memory (the kernel's high-water mark where the case wrote its
summary), the cgroup's peak (resident plus the page cache the worker read), peak private
writable memory (what RLIMIT_DATA counts) and address space, the most threads and open files,
CPU while playing (the last 60 s of samples, in CPUs: 1.0 is one core busy), and the video
frames the client received.
"""
import csv
import json
import sys
from pathlib import Path

root = Path(sys.argv[1] if len(sys.argv) > 1 else "test-results/worker-limits")
print("| Case | Resident | cgroup | Private writable | Address space | Threads | Files | CPU playing | Frames received |")
print("|---|---:|---:|---:|---:|---:|---:|---:|---:|")
for case in sorted(p for p in root.iterdir() if (p / "samples.csv").exists()):
    rows = list(csv.DictReader(open(case / "samples.csv")))
    rows = [r for r in rows if r["rss_kb"] and r["cpu_s"]]
    if not rows:
        continue
    peak = lambda key: max(int(r[key] or 0) for r in rows)
    mib = lambda kb: f"{kb / 1024:.0f} MiB"
    gib = lambda kb: f"{kb / 1024 / 1024:.1f} GiB"
    # CPU over the last 60 s of samples: the game playing, after it loaded.
    last = float(rows[-1]["t"])
    window = [r for r in rows if float(r["t"]) >= last - 60] or rows
    seconds = float(window[-1]["t"]) - float(window[0]["t"])
    cpu = (float(window[-1]["cpu_s"]) - float(window[0]["cpu_s"])) / seconds if seconds else 0
    # The kernel's own high-water mark and the cgroup's peak, where the sampler lived to write
    # them: samples 200 ms apart can miss a short peak.
    summary = case / "summary.json"
    if summary.exists() and summary.read_text().strip():
        s = json.loads(summary.read_text())
        rss_peak = max(peak("rss_kb"), s["rss_peak_kb"])
        cgroup_peak = max(peak("cgroup_kb"), s["cgroup_peak_kb"])
    else:
        rss_peak, cgroup_peak = peak("rss_kb"), peak("cgroup_kb")
    client = case / "client.json"
    frames = json.loads(client.read_text())["frames"] if client.exists() and client.read_text().strip() else "-"
    print(
        f"| {case.name} | {mib(rss_peak)} | {mib(cgroup_peak)} | {mib(peak('data_kb'))}"
        f" | {gib(peak('vm_kb'))} | {peak('threads')} | {peak('fds')} | {cpu:.2f} | {frames} |"
    )
