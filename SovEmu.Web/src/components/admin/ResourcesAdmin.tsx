// The server's resources: what the host is (CPU, cores, instruction sets, memory, kernel), how
// much of its CPU and memory the container (the server and every game session) is using now,
// and charts of both over the last hour, day, week or month. Web only: the apps never show it.

import { useQuery } from "@tanstack/react-query";
import { Activity, ChartLine, Cpu, Info } from "lucide-react";
import { useState } from "react";
import type { ResourceHistory, ResourceRange, ServerResources } from "../../lib/api";
import { q } from "../../lib/data";
import { formatBytes } from "../../lib/numbers";
import { formatCores, geometry, nearest, type Point, severity, tickLabel, timeTicks } from "../../lib/resources";
import { Progress, Segmented } from "../ui/primitives";
import AdminPage, { AdminSection } from "./AdminPage";

const RANGES: { value: ResourceRange; label: string }[] = [
  { value: "HOUR", label: "Hour" },
  { value: "DAY", label: "Day" },
  { value: "WEEK", label: "Week" },
  { value: "MONTH", label: "Month" },
];

function Host({ host }: { host: ServerResources["host"] }) {
  const cpus =
    host.hostCpus && host.hostCpus !== host.usableCpus
      ? `${host.usableCpus} of ${host.hostCpus} logical CPUs`
      : `${host.usableCpus} logical CPUs`;
  return (
    <dl className="mt-4 grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm">
      <dt className="text-slate-400">CPU</dt>
      <dd>{host.cpuModel ?? "Unknown"}</dd>
      <dt className="text-slate-400">Cores</dt>
      <dd>
        {cpus}
        {host.maxMhz ? ` · up to ${(host.maxMhz / 1000).toFixed(1)} GHz` : ""}
      </dd>
      {host.features.length > 0 && (
        <>
          <dt className="text-slate-400">Instruction sets</dt>
          <dd className="flex flex-wrap gap-1.5">
            {host.features.map((f) => (
              <span key={f} className="rounded bg-white/10 px-1.5 py-0.5 font-mono text-xs uppercase">
                {f.replace("_", ".")}
              </span>
            ))}
          </dd>
        </>
      )}
      <dt className="text-slate-400">Memory</dt>
      <dd>{host.memoryBytes ? formatBytes(host.memoryBytes) : "Unknown"}</dd>
      <dt className="text-slate-400">System</dt>
      <dd>
        {host.arch}
        {host.kernel ? ` · kernel ${host.kernel}` : ""}
      </dd>
    </dl>
  );
}

/** A figure with its meter: the fill is coloured by how near the limit it is, and says so. */
function Meter({
  label,
  value,
  percent,
  detail,
}: {
  label: string;
  value: string;
  percent: number | null;
  detail: string;
}) {
  const level = severity(percent ?? 0);
  return (
    <div className="rounded-xl border border-edge bg-surface/60 p-4">
      <div className="text-sm text-slate-400">{label}</div>
      <div className="mt-1 flex items-baseline gap-2">
        <span className="text-3xl font-semibold">{value}</span>
        {level.label && <span className="text-sm font-medium text-amber-300">{level.label}</span>}
      </div>
      <Progress className="mt-3" value={percent ?? 0} color={level.color} label={label} />
      <div className="mt-2 text-sm text-slate-400">{detail}</div>
    </div>
  );
}

function Usage({ r }: { r: ServerResources }) {
  const u = r.usage;
  if (!u) return <p className="mt-4 text-sm text-slate-400">{r.usageNote}</p>;
  const cores = `of ${formatCores(u.allowedCores)} ${u.cpuLimited ? "(the container's quota)" : "(no quota: the host's)"}`;
  const memory = `${formatBytes(u.memoryUsedBytes)} of ${formatBytes(u.memoryLimitBytes)} ${u.memoryLimited ? "(the container's limit)" : "(no limit: the host's)"}`;
  return (
    <div className="mt-4 grid gap-4 sm:grid-cols-2">
      <Meter
        label="CPU"
        value={u.cpuPercent === null ? "…" : `${Math.round(u.cpuPercent)}%`}
        percent={u.cpuPercent}
        detail={cores}
      />
      <Meter
        label="Memory"
        value={u.memoryPercent === null ? "…" : `${Math.round(u.memoryPercent)}%`}
        percent={u.memoryPercent}
        detail={memory}
      />
    </div>
  );
}

const PLOT_H = 140;

/** One resource over the window: each bucket's average as a line, its peak as a faint area. */
function Chart({
  title,
  subtitle,
  points,
  history,
}: {
  title: string;
  subtitle: string;
  points: Point[];
  history: ResourceHistory;
}) {
  const [hover, setHover] = useState<Point | null>(null);
  const window = { from: history.from, to: history.to, bucketSeconds: history.bucketSeconds };
  const g = geometry(points, window, 1000, PLOT_H);
  const span = history.to - history.from;
  const pct = (at: number) => `${((at + history.bucketSeconds / 2 - history.from) / span) * 100}%`;
  const when = (at: number) =>
    new Date(at * 1000).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
  return (
    <figure className="mt-5">
      <figcaption className="flex flex-wrap items-baseline justify-between gap-2">
        <span>
          <span className="font-semibold">{title}</span> <span className="text-sm text-slate-400">{subtitle}</span>
        </span>
        <span className="flex items-center gap-4 text-xs text-slate-400">
          <span className="flex items-center gap-1.5">
            <span className="h-0.5 w-4 rounded bg-accent" /> Average
          </span>
          <span className="flex items-center gap-1.5">
            <span className="h-3 w-4 rounded-sm bg-accent/20" /> Peak
          </span>
        </span>
      </figcaption>
      <div className="mt-2 grid grid-cols-[2.5rem_1fr] gap-2">
        <div className="relative text-right text-xs text-slate-500 tabular-nums" style={{ height: PLOT_H }}>
          <span className="absolute top-0 right-0 -translate-y-1/2">100%</span>
          <span className="absolute top-1/2 right-0 -translate-y-1/2">50%</span>
          <span className="absolute bottom-0 right-0 translate-y-1/2">0%</span>
        </div>
        <div className="relative" style={{ height: PLOT_H }}>
          <svg
            viewBox={`0 0 1000 ${PLOT_H}`}
            preserveAspectRatio="none"
            className="absolute inset-0 h-full w-full overflow-visible"
            role="img"
            aria-label={`${title} over the window: see the table below for the figures`}
            onPointerMove={(e) => {
              const box = e.currentTarget.getBoundingClientRect();
              const at = history.from + ((e.clientX - box.left) / box.width) * span - history.bucketSeconds / 2;
              setHover(nearest(points, at));
            }}
            onPointerLeave={() => setHover(null)}
          >
            {[0, PLOT_H / 2, PLOT_H].map((y) => (
              <line
                key={y}
                x1={0}
                x2={1000}
                y1={y}
                y2={y}
                className="stroke-white/10"
                vectorEffect="non-scaling-stroke"
              />
            ))}
            <path d={g.area} className="fill-accent/15" />
            <path
              d={g.line}
              className="fill-none stroke-accent"
              strokeWidth={2}
              strokeLinejoin="round"
              strokeLinecap="round"
              vectorEffect="non-scaling-stroke"
            />
          </svg>
          {g.dots.map((d) => (
            <span
              key={`${d.x},${d.y}`}
              className="pointer-events-none absolute size-2 -translate-x-1/2 -translate-y-1/2 rounded-full bg-accent ring-2 ring-panel"
              style={{ left: `${d.x / 10}%`, top: d.y }}
            />
          ))}
          {points.length === 0 && (
            <p className="absolute inset-0 grid place-items-center text-sm text-slate-500">
              No readings in this window yet.
            </p>
          )}
          {hover && (
            <>
              <span
                className="pointer-events-none absolute inset-y-0 w-px bg-white/30"
                style={{ left: pct(hover.at) }}
              />
              <span
                className="pointer-events-none absolute size-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-accent ring-2 ring-panel"
                style={{ left: pct(hover.at), top: PLOT_H - (hover.average / 100) * PLOT_H }}
              />
              <div
                className="pointer-events-none absolute top-0 z-10 -translate-x-1/2 -translate-y-full rounded-lg border border-edge bg-panel px-3 py-2 text-xs whitespace-nowrap shadow-xl"
                style={{ left: pct(hover.at) }}
              >
                <div className="text-slate-400">{when(hover.at)}</div>
                <div className="tabular-nums">
                  Average <span className="font-semibold">{hover.average.toFixed(1)}%</span> · Peak{" "}
                  <span className="font-semibold">{hover.max.toFixed(1)}%</span>
                </div>
              </div>
            </>
          )}
        </div>
        <div />
        <div className="relative h-4 text-xs text-slate-500 tabular-nums">
          {timeTicks(history.from, history.to).map((t) => (
            <span
              key={t}
              className="absolute -translate-x-1/2 whitespace-nowrap"
              style={{ left: `${((t - history.from) / span) * 100}%` }}
            >
              {tickLabel(t, span)}
            </span>
          ))}
        </div>
      </div>
      {points.length > 0 && (
        <details className="mt-3 text-sm">
          <summary className="cursor-pointer text-slate-400 hover:text-white">Show as a table</summary>
          <div className="mt-2 max-h-64 overflow-y-auto rounded-lg border border-edge">
            <table className="w-full text-left tabular-nums">
              <thead className="sticky top-0 bg-panel text-slate-400">
                <tr>
                  <th className="px-3 py-1.5 font-medium">From</th>
                  <th className="px-3 py-1.5 text-right font-medium">Average</th>
                  <th className="px-3 py-1.5 text-right font-medium">Peak</th>
                </tr>
              </thead>
              <tbody>
                {[...points].reverse().map((p) => (
                  <tr key={p.at} className="border-t border-white/5">
                    <td className="px-3 py-1">{when(p.at)}</td>
                    <td className="px-3 py-1 text-right">{p.average.toFixed(1)}%</td>
                    <td className="px-3 py-1 text-right">{p.max.toFixed(1)}%</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </details>
      )}
    </figure>
  );
}

function Resources() {
  const [range, setRange] = useState<ResourceRange>("HOUR");
  const resources = useQuery({
    ...q.serverResources(),
    refetchInterval: (query) => Math.max(2, query.state.data?.sampleSeconds || 5) * 1000,
  });
  // The hour's chart moves with each reading; the longer ones with each minute's row.
  const history = useQuery({ ...q.resourceHistory(range), refetchInterval: range === "HOUR" ? 15_000 : 60_000 });

  const r = resources.data;
  if (!r) return null;
  const series = (kind: "CPU" | "MEMORY") => history.data?.series.find((s) => s.resource === kind)?.points ?? [];
  const u = r.usage;
  return (
    <>
      <AdminSection title="Host" icon={<Cpu />}>
        <Host host={r.host} />
      </AdminSection>

      <AdminSection title="In use now" icon={<Activity />}>
        <p className="mt-2 text-sm text-slate-400">
          The container's whole use: the server and every game session running in it.
        </p>
        <Usage r={r} />
      </AdminSection>

      <AdminSection
        title="History"
        icon={<ChartLine />}
        actions={<Segmented label="Time range" value={range} options={RANGES} onChange={setRange} />}
      >
        {history.data ? (
          <>
            <Chart
              title="CPU"
              subtitle={u ? `share of ${formatCores(u.allowedCores)}` : "share of the allowed cores"}
              points={series("CPU")}
              history={history.data}
            />
            <Chart
              title="Memory"
              subtitle={u ? `share of ${formatBytes(u.memoryLimitBytes)}` : "share of the limit"}
              points={series("MEMORY")}
              history={history.data}
            />
          </>
        ) : (
          <p className="mt-4 text-sm text-slate-400">Loading…</p>
        )}
        <p className="mt-4 flex items-start gap-2 text-sm text-slate-400">
          <Info className="mt-0.5 size-4 shrink-0" />
          <span>
            Read every {r.sampleSeconds || "—"} s (<code>SOVEMU_RESOURCE_SAMPLE_SECONDS</code>); a minute's average and
            peak are kept for <code>SOVEMU_RESOURCE_HISTORY_DAYS</code> days.
          </span>
        </p>
      </AdminSection>
    </>
  );
}

export default function ResourcesAdmin() {
  return (
    <AdminPage>
      <Resources />
    </AdminPage>
  );
}
