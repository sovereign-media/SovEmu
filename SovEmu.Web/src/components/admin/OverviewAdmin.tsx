// Overview: the server at a glance, the admin menu's first page. Each card sums up one page and
// links to it: who is playing and the game ports in use, the library and its last scan, game
// information, BIOS gaps, rendering, the disk, the version and uptime, and what went wrong
// lately. One read for them all, again every 30 s.

import { useQuery } from "@tanstack/react-query";
import type { LucideIcon } from "lucide-react";
import { Bug, Cpu, HardDrive, Info, Library, Microchip, MonitorCog, MonitorPlay, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { q } from "../../lib/data";
import { biosGaps, diskText, diskUsed, scanText, uptimeText } from "../../lib/overview";
import { cn } from "../ui/cn";
import { Progress } from "../ui/primitives";
import AdminPage, { ErrorNote } from "./AdminPage";

type CardProps = {
  href: string;
  icon: LucideIcon;
  title: string;
  value: ReactNode;
  warn?: string;
  children?: ReactNode;
};

/** One card: a page's summary, linking to it. `warn` marks it, with a word as well as a colour. */
function Card({ href, icon: Icon, title, value, warn, children }: CardProps) {
  return (
    <a
      href={href}
      className={cn(
        "grid content-start gap-2 rounded-2xl border bg-panel/80 p-5 transition hover:bg-white/5 focus-visible:outline-2",
        warn ? "border-amber-500/40" : "border-white/10",
      )}
    >
      <span className="flex items-center gap-2 text-sm text-slate-400">
        <Icon aria-hidden className="size-4" /> {title}
      </span>
      <span className="text-2xl font-black tabular-nums">{value}</span>
      {warn && (
        <span className="flex items-center gap-1.5 text-sm text-amber-200">
          <TriangleAlert aria-hidden className="size-4 shrink-0" /> {warn}
        </span>
      )}
      {children && <span className="text-sm text-slate-400">{children}</span>}
    </a>
  );
}

function Cards() {
  const overview = useQuery({ ...q.overview(), refetchInterval: 30_000 });
  const o = overview.data;
  if (!o) return <ErrorNote error={overview.error} />;

  const playing = o.activeSessions.length;
  const scan = scanText(o.jobs);
  const gaps = biosGaps(o.systems);
  const used = diskUsed(o.storage);
  const failures = o.sessionFailures.length;
  const clientErrors = o.clientEventGroups.reduce((n, g) => n + g.count, 0);
  const lookup = o.metadataStatus;
  return (
    <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      <Card
        href="/admin/sessions/"
        icon={MonitorPlay}
        title="Playing now"
        value={playing}
        warn={o.network.portsInUse >= o.network.portCount ? "Every game port is in use" : undefined}
      >
        {playing
          ? o.activeSessions.map((s) => `${s.username ?? "someone"}: ${s.title}`).join(" · ")
          : "Nobody is playing."}{" "}
        {o.network.portsInUse} of {o.network.portCount} game ports in use.
      </Card>
      <Card
        href="/admin/"
        icon={Library}
        title="Library"
        value={`${o.libraryStatus.games.toLocaleString("en-US")} games`}
        warn={scan.failed ? scan.text : undefined}
      >
        {scan.failed ? "See the Jobs page for why." : scan.text}
      </Card>
      <Card href="/admin/" icon={Info} title="Game information" value={lookup.running ? "Looking up" : "Idle"}>
        {lookup.total ? `The last lookup found ${lookup.found} of ${lookup.total} games.` : "No lookup has run yet."}
      </Card>
      <Card
        href="/admin/bios/"
        icon={Microchip}
        title="BIOS"
        value={gaps.length ? `${gaps.length} missing` : "Complete"}
        warn={gaps.length ? `${gaps.join(", ")} cannot play every game` : undefined}
      >
        {gaps.length ? undefined : "Every system has the files it needs."}
      </Card>
      <Card
        href="/admin/rendering/"
        icon={MonitorCog}
        title="3D systems"
        value={o.gpu.mode === "GPU" ? "On a GPU" : o.gpu.mode === "SOFTWARE" ? "In software" : "Off"}
        warn={o.gpu.mode === "SOFTWARE" ? "Rendering in software is slow" : undefined}
      >
        {o.gpu.renderer ?? (o.gpu.mode === "OFF" ? "3D systems cannot be played." : undefined)}
      </Card>
      <Card
        href="/admin/storage/"
        icon={HardDrive}
        title="Disk"
        value={used == null ? "Unknown" : `${used}% used`}
        warn={used != null && used >= 90 ? "Nearly full" : undefined}
      >
        {diskText(o.storage)}
        {used != null && <Progress value={used} label="Disk used" />}
      </Card>
      <Card
        href="/admin/sessions/"
        icon={TriangleAlert}
        title="Failed games"
        value={failures}
        warn={failures ? "Games failed since the server started" : undefined}
      >
        {failures ? "See the Sessions page for why." : "No game has failed since the server started."}
      </Card>
      <Card href="/admin/client-errors/" icon={Bug} title="Client errors today" value={clientErrors}>
        {clientErrors ? "What the apps reported in the last day." : "The apps reported nothing in the last day."}
      </Card>
      <Card href="/admin/about/" icon={Cpu} title="Server" value={`SovEmu ${o.buildInfo.version}`}>
        Up {uptimeText(o.serverResources.startedAt)}
        {o.buildInfo.revision ? ` · ${o.buildInfo.revision.slice(0, 7)}` : " · local build"}
      </Card>
    </div>
  );
}

export default function OverviewAdmin() {
  return (
    <AdminPage width="max-w-6xl">
      <Cards />
    </AdminPage>
  );
}
