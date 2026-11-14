// Library administration: where games come from (direct / cache / mirror), the mirror's sync,
// rescans, and fetching game information from metadata services.
import { CloudDownload, FolderSync, HardDrive, Info, RefreshCw, ScanSearch, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { api, formatSize, type LibraryStatus, type MetadataStatus } from "../../lib/api";
import Shell from "../Shell";
import TopBar from "../TopBar";
import { Button } from "../ui/button";
import { Confirm, panel, Progress } from "../ui/primitives";

const MODES: Record<LibraryStatus["mode"], [string, string]> = {
  direct: ["Direct", "Games are played straight from the library: local disk or a share on the same network."],
  cache: ["Cache", "The library is a remote share. Games and artwork are kept on local disk once used, so they start fast next time."],
  mirror: ["Mirror", "A full local copy of the library is kept in sync in the background, and games are played from it."],
};

function Card({ title, icon, actions, children }: { title: string; icon: ReactNode; actions?: ReactNode; children: ReactNode }) {
  return (
    <section className={panel}>
      <div className="flex flex-wrap items-center justify-between gap-4">
        <h2 className="flex items-center gap-2 text-xl font-bold [&_svg]:size-5 [&_svg]:text-slate-400">
          {icon}
          {title}
        </h2>
        <div className="flex gap-2">{actions}</div>
      </div>
      {children}
    </section>
  );
}

function syncText(m: NonNullable<LibraryStatus["mirror"]>): string {
  if (m.error) return m.error;
  if (m.running)
    return `Copying ${m.files_done.toLocaleString()} of ${m.files_total.toLocaleString()} files (${formatSize(m.bytes_done)} of ${formatSize(m.bytes_total)})`;
  if (m.finished_at)
    return `Up to date as of ${new Date(m.finished_at * 1000).toLocaleString()}${m.failed ? ` · ${m.failed} failed` : ""}${m.removed ? ` · ${m.removed} removed` : ""}`;
  return "Not synced yet.";
}

function Library() {
  const [library, setLibrary] = useState<LibraryStatus | null>(null);
  const [metadata, setMetadata] = useState<MetadataStatus | null>(null);
  const [scanning, setScanning] = useState(false);
  const [confirm, setConfirm] = useState(false);

  const refresh = useCallback(async () => {
    const [l, m] = await Promise.all([api.libraryStatus(), api.metadataStatus()]);
    setLibrary(l);
    setMetadata(m);
  }, []);

  useEffect(() => {
    api.status().then((s) => !s.user?.is_admin && location.replace("/"));
    void refresh();
    const t = setInterval(refresh, 2000);
    return () => clearInterval(t);
  }, [refresh]);

  const scan = async () => {
    setScanning(true);
    await api.scan().catch(() => {});
    setScanning(false);
    await refresh();
  };
  const fetchMetadata = async (force: boolean) => {
    await api.fetchMetadata(undefined, force);
    setTimeout(refresh, 300);
  };

  if (!library || !metadata) return null;
  const [mode, help] = MODES[library.mode];
  const m = library.mirror;
  const syncPct = m ? (m.bytes_total ? (100 * m.bytes_done) / m.bytes_total : m.running ? 0 : 100) : 0;
  const metaPct = metadata.total ? (100 * metadata.done) / metadata.total : 0;
  return (
    <main className="mx-auto grid max-w-4xl gap-6 px-6 pt-24 pb-16">
      <Card
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
          {library.cache_bytes !== null && (
            <>
              <dt className="text-slate-400">Local cache</dt>
              <dd>{formatSize(library.cache_bytes)}</dd>
            </>
          )}
        </dl>
        <p className="mt-3 text-sm text-slate-400">{help}</p>
      </Card>

      {m && (
        <Card
          title="Mirror"
          icon={<FolderSync />}
          actions={
            <Button onClick={async () => (await api.syncLibrary(), refresh())} disabled={m.running}>
              <RefreshCw className={m.running ? "animate-spin" : ""} /> Sync now
            </Button>
          }
        >
          <Progress className="mt-4" value={syncPct} label="Mirror sync" />
          <p className="mt-2 text-sm text-slate-300">{syncText(m)}</p>
        </Card>
      )}

      <Card
        title="Game information"
        icon={<Info />}
        actions={
          <>
            <Button onClick={() => fetchMetadata(false)} disabled={metadata.running}>
              <CloudDownload /> Fetch missing
            </Button>
            <Button variant="outline" onClick={() => setConfirm(true)} disabled={metadata.running}>
              <RefreshCw /> Refresh all
            </Button>
          </>
        }
      >
        <p className="mt-3 text-sm text-slate-400">
          Titles, descriptions, release dates, developers, genres and box art. Arcade and Neo Geo names come from FinalBurn
          Neo's own lists. ScreenScraper and TheGamesDB need credentials, set on the server (<code>SOVEMU_SCREENSCRAPER_*</code>
          , <code>SOVEMU_THEGAMESDB_KEY</code>).
        </p>
        <p className="mt-3 text-sm">
          Sources: <span className="font-semibold">{metadata.sources.join(", ")}</span>
        </p>
        <Progress className="mt-4" value={metaPct} color="#10b981" label="Game information" />
        <p className="mt-2 text-sm text-slate-300">
          {metadata.running
            ? `Looking up ${metadata.done.toLocaleString()} of ${metadata.total.toLocaleString()} games…`
            : metadata.done
              ? `Last run: ${metadata.found} found, ${metadata.not_found} not found, ${metadata.failed} to retry.`
              : ""}
        </p>
        {metadata.notes.length > 0 && (
          <ul className="mt-2 grid gap-1 text-sm text-amber-300">
            {metadata.notes.map((n) => (
              <li key={n} className="flex items-start gap-2">
                <TriangleAlert className="mt-0.5 size-4 shrink-0" /> {n}
              </li>
            ))}
          </ul>
        )}
      </Card>

      <Confirm
        open={confirm}
        onOpenChange={setConfirm}
        title="Look every game up again?"
        description="This uses your metadata services' daily quota."
        action="Refresh all"
        onAction={() => fetchMetadata(true)}
      />
    </main>
  );
}

export default function LibraryAdmin() {
  return (
    <Shell>
      <TopBar crumb="Library" />
      <Library />
    </Shell>
  );
}
