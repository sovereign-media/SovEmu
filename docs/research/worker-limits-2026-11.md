# What a game session uses, and the limits set from it

Measurements for sovereign-media/SovEmu#282, taken on 2026-11-27: what one game session's worker
uses on each system, played the way the server plays it, so each system's default limits stop a
core that leaks or spins without ever stopping a game that is fine. The numbers per case are in
[`worker-limits-2026-11/results.md`](worker-limits-2026-11/results.md), every 200 ms sample in
[`samples/`](worker-limits-2026-11/samples/).

## Summary: limit memory held, per system, at several times what was measured

**A 2D session holds 80–150 MiB** with rewind's 60 s of history full (its 64 MiB cap), run-ahead
and the 2× pre-scale on, and the encoder running: NES, SNES, Game Boy, Game Boy Color, GBA, Mega
Drive, PC Engine and PlayStation alike. It runs 12–16 threads, opens 23–24 files and uses at most
0.7 of a CPU, measured under x86 emulation, so less on a real x86 host.

**A 3D session holds 300–450 MiB** with Mesa's software renderer (llvmpipe), 33–49 threads on 12
CPUs (llvmpipe starts one per CPU) and 31–33 files: Dreamcast playing, GameCube booting.

**Address space is no measure of either.** Every session reserves several times what it holds: a
2D game 1 GiB of address space and 450 MiB of private writable memory (what `RLIMIT_DATA` counts)
for 110 MiB resident; Dolphin, with its fast-memory arena, 82 GiB and 67 GiB for 450 MiB. An
`RLIMIT_AS` or `RLIMIT_DATA` low enough to catch a leak would stop GameCube from starting, so the
memory limit is on memory held, resident plus swapped out, watched by the worker itself.

**A runaway core looks like this**: the N64 test suite (a release candidate) does not run in
Mupen64Plus-Next 1.22.2. Under its cached interpreter it spins inside the core on one thread and
holds 2.4 GiB within 17 s, while the stream loop sits stuck (the watchdog logs it). The N64's
default limit, 2 GiB, ends it.

Defaults (`System::default_*` in `SovEmu.Library/src/systems.rs`; `SovEmu__Sessions__Limits__*`
for every system, `SovEmu__Systems__<id>__Limits__*` for one):

| Systems | Memory | Threads | CPUs | Open files | Measured |
|---|---:|---:|---:|---:|---|
| 2D systems, PlayStation | 1024 MiB | 64 | 2 | 1024 | 150 MiB, 16 threads, 0.7 CPU, 24 files |
| N64, Dreamcast | 2048 MiB | 256 | none | 1024 | 300 MiB, 38 threads, 31 files |
| GameCube, PS2 | 4096 MiB | 256 | none | 1024 | 450 MiB booting, 49 threads, 33 files; PS2 not measured |

- **Memory** is the one limit every install gets: over it the session saves and ends (the test
  `a_leaking_core_is_stopped_by_its_memory_limit_and_its_saves_are_written`, on Linux).
- **The 3D systems have no CPU limit**: without a GPU they render on every CPU there is, and a
  cap would make a playable game slow. A spinning thread takes one CPU, and with cgroups the
  kernel shares CPU between sessions fairly anyway.
- **Threads and CPUs need a cgroup** delegated to the server, which a stock Docker or Kubernetes
  container does not have (below). Their defaults matter only where there is one.
- **PS2 is not measured**: there is no freely licensed PS2 BIOS or test content. Its 4 GiB is a
  guess from PCSX2's 32 MiB of main memory and the GameCube's numbers; raise it with
  `SovEmu__Systems__ps2__Limits__MemoryMegabytes` if a game ends over its limit.

## How it was measured

**Sessions.** `scripts/worker-limits/run.sh` runs the image's `sovemu-worker serve` (the same
stream loop as `session`) in its own container, started as the server starts a game: rewind at
60 s and 64 MiB, run-ahead 1 and the 2× pre-scale where the system has them, the system's core
options, VP8, and `/dev/shm` at 1 GiB as `compose.yaml` has it. A headless Chromium on the host
(`client.mjs`) connects over WebRTC as the web client does, receives the video, and presses
buttons on a schedule to move through each test suite's menus, for 90 s. `sample.sh`, inside the
container, reads `/proc/<pid>/status` and `/fd` every 200 ms (resident and private writable
memory, address space, threads, open files, CPU) and the container's cgroup (resident memory plus
the page cache the worker read). `report.py` writes the table.

**Content**: the free ROMs of `scripts/fetch-free-roms.sh`, the 240p Test Suite on every system
(this added SNES, N64, Dreamcast and GameCube builds of it), plus *Nova the Squirrel*, *Castle
Platformer* and *uCity* for gameplay.

**Machine**: Apple M4 Pro, Docker Desktop's Linux VM (12 CPUs, 7.7 GiB, kernel 7.0.14), the
image `ghcr.io/sovereign-media/sovemu:bleeding` at `f58ea78`, run as x86_64 under emulation. No
GPU: 3D cores render with llvmpipe.

## What the emulation changes

Memory, threads and files are the program's own and carry over to an x86 host. **CPU does not**:
every figure here is the emulated program's, several times what it would use natively. And the
emulation broke three cores:

- **PlayStation with run-ahead** crashed (SIGSEGV) 11 s into every run, with the dynarec and with
  the interpreter. Without run-ahead it played the full 90 s, and natively on Apple silicon (the
  arm64 core, run-ahead on) it did not crash, so it is most likely the emulation. Without a BIOS,
  pcsx_rearmed reinitialises its HLE BIOS at every state load, so run-ahead logs a line a frame.
- **N64**: the test suite's boot code (it reports 64 MB of RDRAM) is not one Mupen64Plus-Next
  knows. Its dynarec exits at once ("Compile at bogus memory address"); its cached interpreter
  spins (above). This is the ROM and the core, not the emulation.
- **GameCube** boots, fails to create Dolphin's shader-compiler context under llvmpipe, and then
  waits on itself inside the core, with the JIT and with the interpreter. Its numbers are
  Dolphin's at boot, before a game's textures and shaders.

A real x86 host with a GPU, and real 3D games, will hold more than these suites do. That is why
the 3D defaults are 5–9 times what was measured, against 7 for the 2D ones.

## The cgroup path

Docker mounts the container's cgroup tree read-only. `--security-opt writable-cgroups=true` makes
it writable, but only by root, and the image runs as uid 1000, so a stock container has no
delegated cgroup; nor does a Kubernetes pod. It exists where the server's own cgroup is
delegated to its user (a systemd unit with `Delegate=yes`, rootless Podman). There each worker
gets `memory.max` at 125% of its limit (the worker leaves at 100%, saving; the kernel kills only
one that could not), `memory.swap.max` 0, `memory.oom.group`, `pids.max` and `cpu.max`. Checked
against the kernel as root with writable cgroups (the ignored test
`the_kernel_kills_a_worker_over_its_memory_and_says_so`): without `memory.swap.max` 0, a
process over `memory.max` on a host with swap (Docker Desktop's VM has 1 GiB) is swapped out
rather than killed. For the same reason the worker's own measure counts its swapped-out memory.
