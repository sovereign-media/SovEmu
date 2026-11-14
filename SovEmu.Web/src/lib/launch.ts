// The loading bar while a game starts: the server's phases (saving the last session, starting
// the emulator, opening the core, reading the game) turned into one bar that only moves forward.
//
// Reading the game is most of a slow start. When the server knows how much it will read the bar
// is exact; when the core reads a disc itself it is not, and the bar eases towards the end of
// that phase instead (always moving, never arriving) while the megabytes read count up.

export type StartPhase = "stopping" | "launching" | "core" | "content" | "ready" | "failed";

export interface StartStatus {
  phase: StartPhase;
  read?: number | null;
  total?: number | null;
  elapsed_ms: number;
  phase_ms: number;
  error?: string | null;
}

/** Each phase's share of the bar, and what it is called. */
const PHASES: Record<StartPhase, { from: number; to: number; label: string; ease: number }> = {
  stopping: { from: 0, to: 0.08, label: "Saving your last game", ease: 3000 },
  launching: { from: 0.08, to: 0.12, label: "Starting the emulator", ease: 1000 },
  core: { from: 0.12, to: 0.18, label: "Loading the emulator", ease: 1500 },
  content: { from: 0.18, to: 0.97, label: "Loading the game", ease: 15000 },
  ready: { from: 1, to: 1, label: "Ready", ease: 1 },
  failed: { from: 0, to: 0, label: "Could not start", ease: 1 },
};

const MB = 1024 * 1024;
const mb = (bytes: number) => (bytes >= 100 * MB ? Math.round(bytes / MB) : Math.round((bytes / MB) * 10) / 10);

export interface LaunchView {
  label: string;
  /** Megabytes and seconds, or the error. */
  detail: string;
  /** 0..1 along the whole start. */
  fraction: number;
}

export function describeStart(s: StartStatus): LaunchView {
  const p = PHASES[s.phase];
  if (s.phase === "failed") return { label: p.label, detail: s.error ?? "", fraction: 0 };
  const read = s.read ?? null;
  const total = s.total ?? null;
  const within =
    total && total > 0 && read !== null
      ? Math.min(1, read / total)
      : 1 - Math.exp(-s.phase_ms / p.ease);
  const parts: string[] = [];
  if (s.phase === "content" && read !== null && read > 0) {
    parts.push(total ? `${mb(Math.min(read, total))} of ${mb(total)} MB` : `${mb(read)} MB read`);
  }
  const seconds = Math.floor(s.elapsed_ms / 1000);
  if (seconds >= 3) parts.push(`${seconds} s`);
  return { label: p.label, detail: parts.join(" · "), fraction: p.from + (p.to - p.from) * within };
}

/** Keeps a bar from ever moving backwards (a phase can report less than the one before). */
export class ForwardOnly {
  private max = 0;
  next(fraction: number): number {
    this.max = Math.max(this.max, Math.min(1, Math.max(0, fraction)));
    return this.max;
  }
}

/** An id for following one start (letters, digits and dashes, as the server requires). */
export function newStartId(): string {
  return crypto.randomUUID();
}
