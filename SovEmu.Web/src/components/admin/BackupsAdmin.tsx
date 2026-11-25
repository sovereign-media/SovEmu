// Backups: the database (accounts, settings, mappings, collections, history), the signing keys
// and every save, one zip file each, kept in the backup folder. "Back up now" runs the Backup job
// (its schedule is SovEmu__Backups__IntervalHours); each backup downloads through a signed URL for
// this admin only, since it holds password hashes and keys. A backup is restored only at
// startup, from SovEmu__Backups__RestoreFrom: the page says how, with the line for each backup.

import { useMutation, useQuery } from "@tanstack/react-query";
import { DatabaseBackup, Download, History, Lock, Trash2 } from "lucide-react";
import { useState } from "react";
import { api, type JobState } from "../../lib/api";
import { restoreLine, scheduleText } from "../../lib/backups";
import { invalidate, q } from "../../lib/data";
import { lastRunText } from "../../lib/jobs";
import { formatBytes } from "../../lib/numbers";
import { Button } from "../ui/button";
import { Confirm, Pill } from "../ui/primitives";
import AdminPage, { AdminSection, CopyButton, EnvLine, ErrorNote } from "./AdminPage";

/**
 * Whether a backup is under way: the job says it runs, or its last run has not ended yet (the
 * run is recorded as ended a moment after the job stops running).
 */
const backingUp = (job: JobState | undefined) => !!job && (job.running || job.lastRun?.outcome === "RUNNING");

function Backups() {
  const jobs = useQuery({
    ...q.jobs(),
    refetchInterval: (query) => (backingUp(query.state.data?.find((j) => j.name === "BACKUP")) ? 1000 : false),
  });
  const job = jobs.data?.find((j) => j.name === "BACKUP");
  const backups = useQuery(q.backups(job?.lastRun?.finishedAt ?? null));
  const [deleting, setDeleting] = useState<string | null>(null);
  const start = useMutation({ mutationFn: () => api.runJob("BACKUP"), onSettled: () => invalidate(["admin", "jobs"]) });
  const remove = useMutation({
    mutationFn: (name: string) => api.deleteBackup(name),
    onSettled: () => invalidate(["admin", "backups"]),
  });
  const b = backups.data;
  const failed = job?.lastRun?.outcome === "FAILED";

  return (
    <>
      <AdminSection
        icon={<DatabaseBackup />}
        title="Backups"
        description="The accounts, sign-ins, settings, controller mappings, collections and history, the signing keys, and every save. The library, its game information and the cache are made again by a scan, so they are left out."
        actions={
          <Button onClick={() => start.mutate()} disabled={start.isPending || backingUp(job)}>
            <DatabaseBackup /> {backingUp(job) ? "Backing up…" : "Back up now"}
          </Button>
        }
      >
        <ErrorNote error={backups.error ?? start.error ?? remove.error} />
        {b && (
          <dl className="mt-4 grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm">
            <dt className="text-slate-400">Folder</dt>
            <dd>
              <EnvLine>{b.dir}</EnvLine>
            </dd>
            <dt className="text-slate-400">Schedule</dt>
            <dd>{scheduleText(b)}</dd>
            {job && (
              <>
                <dt className="text-slate-400">Last backup</dt>
                <dd className={failed ? "text-red-300" : undefined}>
                  {lastRunText(job)}
                  {failed && job.lastRun?.message ? `: ${job.lastRun.message}` : ""}
                </dd>
              </>
            )}
          </dl>
        )}
        <p className="mt-3 text-sm text-amber-200">
          A backup holds every account's password hash and the server's signing keys: keep it as safe as the server.
        </p>
        {b &&
          (b.files.length === 0 ? (
            <p className="mt-4 text-sm text-slate-400">No backups yet.</p>
          ) : (
            <ul aria-label="Backups" className="mt-4 divide-y divide-white/5">
              {b.files.map((f) => (
                <li key={f.name} className="flex flex-wrap items-center gap-x-4 gap-y-2 py-2">
                  <div className="min-w-0 flex-1">
                    <p className="truncate font-mono text-sm">{f.name}</p>
                    <p className="text-sm text-slate-400">
                      {new Date(f.created * 1000).toLocaleString()} · {formatBytes(f.bytes)}
                    </p>
                  </div>
                  <Button asChild variant="outline">
                    <a href={f.url} download={f.name}>
                      <Download /> Download
                    </a>
                  </Button>
                  <CopyButton text={restoreLine(b, f.name)} label="Copy restore line" />
                  <Button variant="outline" aria-label={`Delete ${f.name}`} onClick={() => setDeleting(f.name)}>
                    <Trash2 />
                  </Button>
                </li>
              ))}
            </ul>
          ))}
      </AdminSection>

      <AdminSection
        icon={<History />}
        title="Restoring a backup"
        description="A running server's database cannot be replaced safely, so a backup is restored as the server starts."
      >
        {b?.restoreFrom && (
          <p className="mt-4 flex flex-wrap items-center gap-2 text-sm">
            <Pill tone="accent">
              <Lock aria-hidden /> Set on the server
            </Pill>
            Restored at startup from <EnvLine>{b.restoreFrom}</EnvLine>, once: remove the setting when you are done.
          </p>
        )}
        <ol className="mt-4 grid list-decimal gap-2 pl-5 text-sm text-slate-300">
          <li>
            Set <EnvLine>SovEmu__Backups__RestoreFrom</EnvLine> to the backup's path, as the server sees it ("Copy
            restore line" gives it for a backup here; a downloaded one can be put anywhere the server can read).
          </li>
          <li>
            Restart the server. Before its database opens it moves the database, the signing keys and the saves into{" "}
            <EnvLine>before-restore-&lt;time&gt;</EnvLine> in the data folder, never deleting them, and puts the
            backup's in their place.
          </li>
          <li>Remove the setting. Until then a restart restores nothing more: each backup is restored once.</li>
        </ol>
      </AdminSection>

      <Confirm
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={`Delete ${deleting}?`}
        description="It cannot be restored once deleted."
        action="Delete"
        onAction={() => deleting && remove.mutate(deleting)}
      />
    </>
  );
}

export default function BackupsAdmin() {
  return (
    <AdminPage width="max-w-5xl">
      <Backups />
    </AdminPage>
  );
}
