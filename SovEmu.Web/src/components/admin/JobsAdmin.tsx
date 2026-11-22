// Background jobs: what the server does on its own (scans, mirror syncs, game information,
// database upkeep), when each runs next, how each last went, and a button to run one now. The
// schedules are settings (`SOVEMU_*_INTERVAL_MINUTES`); the button is a shortcut.

import { useMutation, useQuery } from "@tanstack/react-query";
import { CircleAlert, CircleCheck, Clock, LoaderCircle, Play } from "lucide-react";
import { api, type JobState } from "../../lib/api";
import { invalidate, q } from "../../lib/data";
import { lastRunText, scheduleText } from "../../lib/jobs";
import { Button } from "../ui/button";
import AdminPage, { AdminSection, ErrorNote } from "./AdminPage";

function StatusIcon({ job }: { job: JobState }) {
  if (job.running) return <LoaderCircle className="size-5 animate-spin text-sky-400" aria-label="Running" />;
  const outcome = job.lastRun?.outcome;
  if (outcome === "SUCCEEDED") return <CircleCheck className="size-5 text-emerald-400" aria-label="Succeeded" />;
  if (outcome === "FAILED" || outcome === "INTERRUPTED")
    return <CircleAlert className="size-5 text-amber-400" aria-label="Failed" />;
  return <Clock className="size-5 text-slate-500" aria-label="Not run yet" />;
}

function Jobs() {
  // Runs end on the server: their state is read every 2 s.
  const jobs = useQuery({ ...q.jobs(), refetchInterval: 2000 });

  // A scan or a lookup changes what the library pages show.
  const run = useMutation({
    mutationFn: api.runJob,
    onSettled: () => invalidate(["admin"]),
  });

  if (!jobs.data) return null;
  const now = Date.now();
  return (
    <>
      <AdminSection
        title="Background jobs"
        description={
          <>
            What the server does on its own. Intervals are set on the server (<code>SOVEMU_SCAN_INTERVAL_MINUTES</code>,{" "}
            <code>SOVEMU_MIRROR_INTERVAL_MINUTES</code>, <code>SOVEMU_MAINTENANCE_INTERVAL_MINUTES</code>,{" "}
            <code>SOVEMU_BACKUP_INTERVAL_HOURS</code>); 0 turns a job's schedule off.
          </>
        }
      >
        <div className="mt-3">
          <ErrorNote error={run.error} />
        </div>
        <ul className="mt-4 divide-y divide-white/10">
          {jobs.data.map((job) => (
            <li key={job.name} className="flex items-start gap-4 py-4">
              <StatusIcon job={job} />
              <div className="min-w-0 flex-1">
                <div className="font-semibold">{job.label}</div>
                <p className="text-sm text-slate-400">{job.description}</p>
                <p className="mt-1 text-sm text-slate-300">
                  {scheduleText(job, now)} · {lastRunText(job, now)}
                </p>
                {!job.running && job.lastRun?.message && (
                  <p
                    className={`mt-1 text-sm break-words ${job.lastRun.outcome === "FAILED" ? "text-amber-300" : "text-slate-400"}`}
                  >
                    {job.lastRun.message}
                  </p>
                )}
              </div>
              <Button
                variant="outline"
                onClick={() => run.mutate(job.name)}
                disabled={job.running || (run.isPending && run.variables === job.name)}
              >
                <Play /> Run now
              </Button>
            </li>
          ))}
        </ul>
      </AdminSection>
    </>
  );
}

export default function JobsAdmin() {
  return (
    <AdminPage>
      <Jobs />
    </AdminPage>
  );
}
