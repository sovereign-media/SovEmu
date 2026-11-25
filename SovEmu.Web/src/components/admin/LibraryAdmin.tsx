// Library administration: where games come from (direct / cache / mirror), the mirror's sync,
// rescans, and fetching game information from metadata services.

import { useMutation, useQuery } from "@tanstack/react-query";
import {
  CircleCheck,
  CircleDashed,
  CircleX,
  CloudDownload,
  FolderSync,
  HardDrive,
  ImageDown,
  Info,
  RefreshCw,
  ScanSearch,
  TriangleAlert,
} from "lucide-react";
import { useCallback, useState } from "react";
import { api, type LibraryStatus, type MetadataField, type MetadataStatus } from "../../lib/api";
import { invalidate, q } from "../../lib/data";
import { accountText, progressText, quotaPercent, quotaText, recentText, workingText } from "../../lib/lookup";
import { formatBytes } from "../../lib/numbers";
import { Button } from "../ui/button";
import { Confirm, Progress } from "../ui/primitives";
import AdminPage, { AdminSection } from "./AdminPage";

const MODES: Record<LibraryStatus["mode"], [string, string]> = {
  DIRECT: ["Direct", "Games are played straight from the library: local disk or a share on the same network."],
  CACHE: [
    "Cache",
    "The library is a remote share. Games and artwork are kept on local disk once used, so they start fast next time.",
  ],
  MIRROR: [
    "Mirror",
    "A full local copy of the library is kept in sync in the background, and games are played from it.",
  ],
};

function syncText(m: NonNullable<LibraryStatus["mirror"]>): string {
  if (m.error) return m.error;
  if (m.running)
    return `Copying ${m.filesDone.toLocaleString()} of ${m.filesTotal.toLocaleString()} files (${formatBytes(m.bytesDone)} of ${formatBytes(m.bytesTotal)})`;
  if (m.finishedAt)
    return `Up to date as of ${new Date(m.finishedAt * 1000).toLocaleString()}${m.failed ? ` · ${m.failed} failed` : ""}${m.removed ? ` · ${m.removed} removed` : ""}`;
  return "Not synced yet.";
}

function Library() {
  // Scans, syncs and lookups run on the server: their progress is read every 2 s.
  const status = useQuery({ ...q.adminStatus(), refetchInterval: 2000 });
  const library: LibraryStatus | null = status.data?.libraryStatus ?? null;
  const metadata: MetadataStatus | null = status.data?.metadataStatus ?? null;
  const [confirm, setConfirm] = useState(false);
  const refresh = useCallback(() => invalidate(["admin", "status"]), []);

  // A rescan changes what every screen lists.
  const scanning_ = useMutation({
    mutationFn: () => api.scan(),
    onSettled: () => invalidate(["admin", "status"], ["home"], ["systems"], ["system"], ["collections"]),
  });
  const scanning = scanning_.isPending;
  const scan = () => scanning_.mutate();
  const fetchMetadata = async (force: boolean, fields: MetadataField[] = []) => {
    await api.fetchMetadata(undefined, force, fields);
    setTimeout(refresh, 300);
  };

  if (!library || !metadata) return null;
  const [mode, help] = MODES[library.mode];
  const m = library.mirror;
  const syncPct = m ? (m.bytesTotal ? (100 * m.bytesDone) / m.bytesTotal : m.running ? 0 : 100) : 0;
  const metaPct = metadata.total ? (100 * metadata.done) / metadata.total : 0;
  const art = metadata.artwork;
  const artPct = art.total ? (100 * art.done) / art.total : 0;
  const working = workingText(metadata);
  return (
    <>
      <AdminSection
        title="Library"
        icon={<HardDrive />}
        actions={
          <Button onClick={scan} disabled={scanning}>
            <ScanSearch /> {scanning ? "Scanning…" : "Rescan"}
          </Button>
        }
      >
        <dl className="mt-4 grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm">
          <dt className="text-slate-400">Mode</dt>
          <dd>{mode}</dd>
          <dt className="text-slate-400">Games</dt>
          <dd>{library.games.toLocaleString()}</dd>
          {library.cacheBytes !== null && (
            <>
              <dt className="text-slate-400">Local cache</dt>
              <dd>{formatBytes(library.cacheBytes)}</dd>
            </>
          )}
        </dl>
        <p className="mt-3 text-sm text-slate-400">{help}</p>
      </AdminSection>

      {m && (
        <AdminSection
          title="Mirror"
          icon={<FolderSync />}
          actions={
            <Button
              onClick={async () => {
                await api.syncLibrary();
                refresh();
              }}
              disabled={m.running}
            >
              <RefreshCw className={m.running ? "animate-spin" : ""} /> Sync now
            </Button>
          }
        >
          <Progress className="mt-4" value={syncPct} label="Mirror sync" />
          <p className="mt-2 text-sm text-slate-300">{syncText(m)}</p>
        </AdminSection>
      )}

      <AdminSection
        title="Game information"
        icon={<Info />}
        actions={
          <>
            <Button onClick={() => fetchMetadata(false)} disabled={metadata.running}>
              <CloudDownload /> Fetch missing
            </Button>
            <Button
              variant="outline"
              onClick={() => fetchMetadata(true, ["BOX_ART", "SCREENSHOT", "MARQUEE"])}
              disabled={metadata.running}
            >
              <ImageDown /> Refresh artwork
            </Button>
            <Button variant="outline" onClick={() => setConfirm(true)} disabled={metadata.running}>
              <RefreshCw /> Refresh all
            </Button>
          </>
        }
      >
        <p className="mt-3 text-sm text-slate-400">
          Titles, descriptions, release dates, developers, genres, ratings, ESRB age ratings, play modes, series,
          themes, box art, screenshots, logos and videos. Arcade and Neo Geo names come from FinalBurn Neo's own lists;
          every other game is matched by its file's hash on ScreenScraper, which needs developer credentials set on the
          server (<code>SovEmu__Metadata__ScreenScraper__*</code>).
        </p>
        <p className="mt-3 text-sm">
          Sources:{" "}
          <span className="font-semibold">
            {metadata.providers.length ? metadata.providers.map((p) => p.name).join(", ") : metadata.sources.join(", ")}
          </span>
        </p>
        <Progress className="mt-4" value={metaPct} color="#10b981" label="Game information" />
        <p className="mt-2 text-sm text-slate-300">{progressText(metadata)}</p>
        {working.length > 0 && (
          <ul className="mt-1 grid gap-0.5 text-sm text-slate-400" aria-label="Looking up now">
            {working.map((w) => (
              <li key={w}>Now: {w}</li>
            ))}
          </ul>
        )}

        {metadata.systems.length > 1 && (
          <div className="mt-4 grid gap-x-6 gap-y-2 sm:grid-cols-2">
            {metadata.systems.map((sys) => (
              <div key={sys.system} className="text-sm">
                <div className="flex justify-between text-slate-400">
                  <span>{sys.system}</span>
                  <span>
                    {sys.done.toLocaleString()} / {sys.total.toLocaleString()}
                  </span>
                </div>
                <Progress
                  className="mt-1"
                  value={sys.total ? (100 * sys.done) / sys.total : 0}
                  color="#10b981"
                  label={`${sys.system} game information`}
                />
              </div>
            ))}
          </div>
        )}

        {metadata.providers.length > 0 && (
          <table className="mt-4 w-full text-left text-sm">
            <thead className="text-slate-400">
              <tr>
                <th className="py-1 font-normal">Source</th>
                <th className="py-1 font-normal">Asked</th>
                <th className="py-1 font-normal">Found</th>
                <th className="py-1 font-normal">Not found</th>
                <th className="py-1 font-normal">Errors</th>
                <th className="py-1 font-normal">Quota</th>
              </tr>
            </thead>
            <tbody>
              {metadata.providers.map((p) => {
                const pct = quotaPercent(p);
                return (
                  <tr key={p.id} className="border-t border-white/5">
                    <td className="py-1">
                      {p.name}
                      {p.account && <div className="text-xs text-slate-400">{accountText(p)}</div>}
                    </td>
                    <td className="py-1">{p.asked.toLocaleString()}</td>
                    <td className="py-1">{p.found.toLocaleString()}</td>
                    <td className="py-1">{p.notFound.toLocaleString()}</td>
                    <td className="py-1">{p.errors.toLocaleString()}</td>
                    <td className="py-1">
                      {quotaText(p)}
                      {pct !== null && (
                        <Progress
                          className="mt-1 max-w-48"
                          value={pct}
                          color={pct > 90 ? "#f59e0b" : "#0ea5e9"}
                          label={`${p.name} quota`}
                        />
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}

        {art.total > 0 && (
          <>
            <Progress className="mt-4" value={artPct} color="#0ea5e9" label="Box art and videos" />
            <p className="mt-2 text-sm text-slate-300">
              {art.running
                ? `Downloading box art and videos in the background: ${art.done.toLocaleString()} of ${art.total.toLocaleString()}, for games without their own…`
                : `Box art and videos: ${art.downloaded.toLocaleString()} downloaded${art.videos ? ` (${art.videos.toLocaleString()} videos)` : ""}${art.failed ? `, ${art.failed.toLocaleString()} failed` : ""}.`}
            </p>
          </>
        )}
        {metadata.notes.length > 0 && (
          <ul className="mt-2 grid gap-1 text-sm text-amber-300">
            {metadata.notes.map((n) => (
              <li key={n} className="flex items-start gap-2">
                <TriangleAlert className="mt-0.5 size-4 shrink-0" /> {n}
              </li>
            ))}
          </ul>
        )}

        {metadata.recent.length > 0 && (
          <>
            <h3 className="mt-4 text-sm font-semibold">Just looked up</h3>
            <ul className="mt-1 grid gap-1 text-sm" aria-label="Games just looked up">
              {metadata.recent.map((r) => (
                <li key={`${r.gameId}-${r.outcome}`} className="flex items-start gap-2">
                  {r.outcome === "FOUND" ? (
                    <CircleCheck className="mt-0.5 size-4 shrink-0 text-emerald-400" />
                  ) : r.outcome === "NOT_FOUND" ? (
                    <CircleDashed className="mt-0.5 size-4 shrink-0 text-slate-400" />
                  ) : (
                    <CircleX className="mt-0.5 size-4 shrink-0 text-amber-300" />
                  )}
                  <span>
                    {r.title} <span className="text-slate-400">({r.system})</span>{" "}
                    <span className="text-slate-400">· {recentText(metadata, r)}</span>
                  </span>
                </li>
              ))}
            </ul>
          </>
        )}
      </AdminSection>

      <Confirm
        open={confirm}
        onOpenChange={setConfirm}
        title="Look every game up again?"
        description="Every game is asked about again, which uses ScreenScraper's daily quota. Nothing a game has is cleared when a source no longer knows it."
        action="Refresh all"
        onAction={() => fetchMetadata(true)}
      />
    </>
  );
}

export default function LibraryAdmin() {
  return (
    <AdminPage>
      <Library />
    </AdminPage>
  );
}
