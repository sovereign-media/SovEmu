// Library administration: where games come from (direct / cache / mirror), the mirror's sync,
// rescans, and fetching game information from metadata services.

import { CloudDownload, FolderSync, HardDrive, Info, RefreshCw, ScanSearch, TriangleAlert } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useState } from "react";
import { api, formatSize, type LibraryStatus, type MetadataStatus } from "../../lib/api";
import { go } from "../../lib/go";
import Shell from "../Shell";
import SideNav, { NAV_SPACE } from "../SideNav";
import { Button } from "../ui/button";
import { Confirm, Progress, panel } from "../ui/primitives";

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

function Card({
  title,
  icon,
  actions,
  children,
}: {
  title: string;
  icon: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
}) {
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
    return `Copying ${m.filesDone.toLocaleString()} of ${m.filesTotal.toLocaleString()} files (${formatSize(m.bytesDone)} of ${formatSize(m.bytesTotal)})`;
  if (m.finishedAt)
    return `Up to date as of ${new Date(m.finishedAt * 1000).toLocaleString()}${m.failed ? ` · ${m.failed} failed` : ""}${m.removed ? ` · ${m.removed} removed` : ""}`;
  return "Not synced yet.";
}

function Library() {
  const [library, setLibrary] = useState<LibraryStatus | null>(null);
  const [metadata, setMetadata] = useState<MetadataStatus | null>(null);
  const [scanning, setScanning] = useState(false);
  const [confirm, setConfirm] = useState(false);

  const refresh = useCallback(async () => {
    const status = await api.adminStatus();
    setLibrary(status.libraryStatus);
    setMetadata(status.metadataStatus);
  }, []);

  useEffect(() => {
    api.viewer().then((s) => !s.user?.isAdmin && go("/", { replace: true }));
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
  const syncPct = m ? (m.bytesTotal ? (100 * m.bytesDone) / m.bytesTotal : m.running ? 0 : 100) : 0;
  const metaPct = metadata.total ? (100 * metadata.done) / metadata.total : 0;
  const art = metadata.artwork;
  const artPct = art.total ? (100 * art.done) / art.total : 0;
  return (
    <main className="mx-auto grid max-w-4xl gap-6 px-6 pt-16 pb-16">
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
          {library.cacheBytes !== null && (
            <>
              <dt className="text-slate-400">Local cache</dt>
              <dd>{formatSize(library.cacheBytes)}</dd>
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
          Titles, descriptions, release dates, developers, genres, ratings and box art. Arcade and Neo Geo names come
          from FinalBurn Neo's own lists, and most games from the LaunchBox Games Database, which needs no account (its
          dump is downloaded by the server). ScreenScraper and TheGamesDB fill the gaps and need credentials, set on the
          server (<code>SOVEMU_SCREENSCRAPER_*</code>, <code>SOVEMU_THEGAMESDB_KEY</code>).
        </p>
        <p className="mt-3 text-sm">
          Sources: <span className="font-semibold">{metadata.sources.join(", ")}</span>
        </p>
        <Progress className="mt-4" value={metaPct} color="#10b981" label="Game information" />
        <p className="mt-2 text-sm text-slate-300">
          {metadata.running
            ? `Looking up ${metadata.done.toLocaleString()} of ${metadata.total.toLocaleString()} games…`
            : metadata.done
              ? `Last run: ${metadata.found} found, ${metadata.notFound} not found, ${metadata.failed} to retry.`
              : ""}
        </p>
        {art.total > 0 && (
          <>
            <Progress className="mt-4" value={artPct} color="#0ea5e9" label="Box art" />
            <p className="mt-2 text-sm text-slate-300">
              {art.running
                ? `Downloading box art in the background: ${art.done.toLocaleString()} of ${art.total.toLocaleString()} games without a local thumb…`
                : `Box art: ${art.downloaded.toLocaleString()} downloaded${art.failed ? `, ${art.failed.toLocaleString()} failed` : ""}.`}
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
      <SideNav section="admin" />
      <div className={NAV_SPACE}>
        <Library />
      </div>
    </Shell>
  );
}
