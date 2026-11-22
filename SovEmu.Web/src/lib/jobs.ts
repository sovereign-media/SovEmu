// The admin Jobs page's wording: what each background job is called, when it runs, and how its
// last run went. Times from the API are Unix seconds.
import type { JobName, JobState } from "./api";

export const JOB_LABELS: Record<JobName, string> = {
  SCAN: "Library scan",
  MIRROR_SYNC: "Mirror sync",
  METADATA: "Game information",
  MAINTENANCE: "Database upkeep",
};

/** "45 s", "12 min", "3 h", "2 days". */
export function formatSpan(seconds: number): string {
  if (seconds < 60) return `${Math.max(0, Math.round(seconds))} s`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h`;
  const days = Math.round(hours / 24);
  return days === 1 ? "1 day" : `${days} days`;
}

/** How long a run took: "0.4 s", "12 s", "3 min". */
export function formatTook(seconds: number): string {
  return seconds < 10 ? `${seconds.toFixed(1)} s` : formatSpan(seconds);
}

/** When the job runs on its own. */
export function scheduleText(job: JobState, now = Date.now()): string {
  if (job.intervalMinutes === null) return job.name === "METADATA" ? "After each scan" : "Not scheduled";
  const every = `Every ${formatSpan(job.intervalMinutes * 60)}`;
  if (job.running || job.nextRunAt === null) return every;
  return `${every} · next in ${formatSpan(job.nextRunAt - now / 1000)}`;
}

/** How the job's latest run went, or that it is running now. */
export function lastRunText(job: JobState, now = Date.now()): string {
  const run = job.lastRun;
  if (job.running) {
    const since = job.runningSince ?? run?.startedAt;
    return since === null || since === undefined ? "Running…" : `Running for ${formatSpan(now / 1000 - since)}…`;
  }
  if (!run) return "Not run yet";
  const outcome = {
    RUNNING: "Running",
    SUCCEEDED: "Succeeded",
    FAILED: "Failed",
    CANCELLED: "Cancelled",
    INTERRUPTED: "Interrupted",
  }[run.outcome];
  const ago = `${formatSpan(now / 1000 - (run.finishedAt ?? run.startedAt))} ago`;
  const took = run.seconds === null ? "" : `, took ${formatTook(run.seconds)}`;
  return `${outcome} ${ago}${took}`;
}
