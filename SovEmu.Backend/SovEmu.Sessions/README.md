# SovEmu.Sessions (`sovemu-sessions`)

Game sessions. Each game runs in its own `sovemu-worker session` process (one libretro core per
process), and this crate starts, drives and stops those processes for the server.

| Module | What |
|---|---|
| `workers` | `Workers`: the running sessions and their UDP ports (`SOVEMU_UDP_PORTS`). A worker gets only the allow-listed environment (`WORKER_ENV`, no secrets), is spoken to over JSON lines on stdin/stdout (`sovemu_protocol::ipc`), and is always asked to shut down, so its saves are written, before it is killed (`Workers::stop`). |
| `starts` | Progress of games being started (reading, loading), for the clients' loading screens |
| `gpu` | Where 3D systems render, from `sovemu-worker gpu` (`SOVEMU_GPU`): a device, software, or off |

It never links a core or a codec (`RetroHost`, `Media`). It knows nothing about HTTP or the
database. Sessions starting and ending are reported to an `Observer`, which the server implements
to record its metrics. Errors are `sovemu_sessions::Error`.

```bash
cargo test -p sovemu-sessions
```
