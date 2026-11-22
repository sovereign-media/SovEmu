// Statistics: what was played over the last week, month or year. The totals as figures, the time
// played per day (or per week, over a year) as one series of bars with a tooltip on each, then
// the most played games, each account's and each system's time, and the most favourited games
// as tables (the chart's figures are in them too, so nothing is read off the bars alone).

import { useQuery } from "@tanstack/react-query";
import { BarChart3 } from "lucide-react";
import { type ReactNode, useState } from "react";
import type { PlayStatistics, StatsRange } from "../../lib/api";
import { q } from "../../lib/data";
import { BUCKET_DAYS, type Bucket, bucketLabel, buckets, playedText, RANGES } from "../../lib/statistics";
import { Segmented, Tooltip } from "../ui/primitives";
import AdminPage, { AdminSection, ErrorNote } from "./AdminPage";

function Figure({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-white/10 bg-black/20 p-4">
      <div className="text-xs tracking-wide text-slate-400 uppercase">{label}</div>
      <div className="mt-1 text-2xl font-black tabular-nums">{value}</div>
    </div>
  );
}

const PLOT_H = 160;

/** Time played per bucket: one series, so no legend; the title names it. */
function TimeChart({ bars }: { bars: Bucket[] }) {
  const max = Math.max(...bars.map((b) => b.seconds), 1);
  return (
    <figure className="mt-5">
      <figcaption className="font-semibold">
        Time played <span className="text-sm font-normal text-slate-400">per {bars[0]?.days > 1 ? "week" : "day"}</span>
      </figcaption>
      <div className="mt-3 grid grid-cols-[3rem_1fr] gap-2">
        <div className="relative text-right text-xs text-slate-500 tabular-nums" style={{ height: PLOT_H }}>
          <span className="absolute top-0 right-0 -translate-y-1/2">{playedText(max)}</span>
          <span className="absolute bottom-0 right-0 translate-y-1/2">0</span>
        </div>
        <div
          className="relative flex items-end gap-[2px] border-b border-white/10"
          style={{ height: PLOT_H }}
          role="img"
          aria-label="Time played over the window: the tables below have the figures"
        >
          <div className="pointer-events-none absolute inset-x-0 top-0 border-t border-white/5" />
          {bars.map((b) => (
            <Tooltip
              key={b.start}
              content={`${bucketLabel(b)}: ${b.sessions ? `${playedText(b.seconds)} in ${b.sessions} ${b.sessions === 1 ? "game" : "games"}` : "nothing played"}`}
            >
              {/* The whole column is the hit target, taller than the bar it holds. */}
              <div className="group flex h-full min-w-0 flex-1 items-end">
                <div
                  className="w-full rounded-t-[4px] bg-accent group-hover:brightness-125"
                  style={{ height: b.seconds ? Math.max(2, (b.seconds / max) * PLOT_H) : 0 }}
                />
              </div>
            </Tooltip>
          ))}
        </div>
      </div>
      <div className="mt-1 ml-[3.5rem] flex justify-between text-xs text-slate-500">
        <span>{bars[0] ? bucketLabel(bars[0]) : ""}</span>
        <span>{bars.length ? bucketLabel(bars[bars.length - 1]) : ""}</span>
      </div>
    </figure>
  );
}

/** A small table: a header row and the rows given, or a line saying there is nothing. */
function Table({ heads, rows, empty }: { heads: string[]; rows: ReactNode[][]; empty: string }) {
  if (rows.length === 0) return <p className="mt-3 text-sm text-slate-400">{empty}</p>;
  return (
    <div className="mt-3 overflow-x-auto">
      <table className="w-full text-left text-sm">
        <thead className="text-xs tracking-wide text-slate-400 uppercase">
          <tr>
            {heads.map((h, i) => (
              <th key={h} className={`py-2 pr-4 font-semibold ${i > 0 ? "text-right" : ""}`}>
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y divide-white/10">
          {rows.map((cells, r) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: rows of one fixed result, in its order
            <tr key={r}>
              {cells.map((c, i) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: the cells of one row
                <td key={i} className={`py-2 pr-4 ${i > 0 ? "text-right tabular-nums text-slate-300" : ""}`}>
                  {c}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

const gameLink = (g: { gameId: number | null; title: string; system: string }) =>
  g.gameId != null ? (
    <a className="font-semibold hover:underline" href={`/system/?id=${encodeURIComponent(g.system)}&game=${g.gameId}`}>
      {g.title}
    </a>
  ) : (
    <span className="font-semibold">{g.title}</span>
  );

function Figures({ s, range }: { s: PlayStatistics; range: StatsRange }) {
  return (
    <>
      <AdminSection icon={<BarChart3 />} title="Played">
        <div className="mt-4 grid grid-cols-2 gap-3 md:grid-cols-4">
          <Figure label="Games played" value={s.totals.sessions.toLocaleString("en-US")} />
          <Figure label="Time played" value={playedText(s.totals.seconds)} />
          <Figure label="Players" value={String(s.totals.players)} />
          <Figure label="Different games" value={String(s.totals.games)} />
        </div>
        <TimeChart bars={buckets(s, BUCKET_DAYS[range])} />
      </AdminSection>
      <AdminSection title="Most played games">
        <Table
          heads={["Game", "Time", "Plays", "Players"]}
          empty="Nothing played in this window."
          rows={s.games.map((g) => [
            <>
              {gameLink(g)} <span className="text-xs text-slate-400">{g.system.toUpperCase()}</span>
            </>,
            playedText(g.seconds),
            g.sessions,
            g.players,
          ])}
        />
      </AdminSection>
      <div className="grid gap-6 lg:grid-cols-2">
        <AdminSection title="By account">
          <Table
            heads={["Account", "Time", "Plays", "Games"]}
            empty="Nobody played in this window."
            rows={s.players.map((p) => [
              <a key="name" className="font-semibold hover:underline" href={`/admin/users/?id=${p.userId}`}>
                {p.username ?? "(deleted account)"}
              </a>,
              playedText(p.seconds),
              p.sessions,
              p.games,
            ])}
          />
        </AdminSection>
        <AdminSection title="By system">
          <Table
            heads={["System", "Time", "Plays", "Players"]}
            empty="Nothing played in this window."
            rows={s.systems.map((y) => [y.system.toUpperCase(), playedText(y.seconds), y.sessions, y.players])}
          />
        </AdminSection>
      </div>
      <AdminSection title="Most favourited" description="Of all time: favourites are not dated.">
        <Table
          heads={["Game", "Favourite of"]}
          empty="No game is anyone's favourite yet."
          rows={s.favourites.map((f) => [gameLink(f), `${f.users} ${f.users === 1 ? "account" : "accounts"}`])}
        />
      </AdminSection>
    </>
  );
}

function Statistics() {
  const [range, setRange] = useState<StatsRange>("WEEK");
  const stats = useQuery(q.playStatistics(range));
  return (
    <>
      <div className="flex justify-end">
        <Segmented label="Window" value={range} options={RANGES} onChange={setRange} />
      </div>
      <ErrorNote error={stats.error} />
      {stats.data && <Figures s={stats.data} range={range} />}
    </>
  );
}

export default function StatisticsAdmin() {
  return (
    <AdminPage width="max-w-5xl">
      <Statistics />
    </AdminPage>
  );
}
