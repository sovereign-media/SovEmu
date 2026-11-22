// Active sessions: every game being played now, who is playing it on what, and how its stream
// is made (the render path, the codec and encoder, any encoder it fell back from and why) with
// the worker's live numbers. Like Sovereign's active streams; read every 2 s. Under them, the
// games that failed lately (to start, or while playing) with the core's last lines.

import { useMutation, useQuery } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, CircleAlert, MonitorPlay, Square, TriangleAlert } from "lucide-react";
import { Fragment, useEffect, useState } from "react";
import { type ActiveSession, api, type SessionFailure } from "../../lib/api";
import { invalidate, q } from "../../lib/data";
import { go } from "../../lib/go";
import { audioLabel, fallbacksLabel, renderingLabel, runAheadLabel, streamLabel } from "../../lib/pipeline";
import {
  clientSummary,
  decoderSummary,
  durationLabel,
  failureLabel,
  frameWorkLabel,
  healthSummary,
} from "../../lib/sessions";
import Shell from "../Shell";
import SideNav, { NAV_SPACE } from "../SideNav";
import { Button } from "../ui/button";
import { Confirm, panel, Tooltip } from "../ui/primitives";

const pill = "inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-xs font-semibold whitespace-nowrap";

/** The encoder pill: green on a GPU, grey on the CPU, with a mark when it fell back. */
function StreamPill({ p }: { p: NonNullable<ActiveSession["pipeline"]> }) {
  const tone = p.videoHardware ? "bg-emerald-500/20 text-emerald-200" : "bg-white/10 text-slate-200";
  return (
    <span className="inline-flex items-center gap-1.5">
      <span className={`${pill} ${tone}`}>{streamLabel(p.videoCodec, p.videoEncoder, p.videoHardware)}</span>
      {p.fallbacks.length > 0 && (
        <Tooltip content={`Fell back from ${fallbacksLabel(p.fallbacks)}`}>
          <CircleAlert className="size-4 text-amber-400" aria-label="Fell back" />
        </Tooltip>
      )}
    </span>
  );
}

function RenderPill({ p }: { p: NonNullable<ActiveSession["pipeline"]> }) {
  if (!p.renderer) return <span className="text-sm text-slate-400">CPU</span>;
  const tone = p.rendererSoftware ? "bg-amber-500/20 text-amber-200" : "bg-emerald-500/20 text-emerald-200";
  return (
    <Tooltip content={renderingLabel(p.renderer, p.rendererSoftware)}>
      <span className={`${pill} ${tone}`}>{p.rendererSoftware ? "Software" : "GPU"}</span>
    </Tooltip>
  );
}

/** Everything known about one session's stream, under its row. */
function Details({ s }: { s: ActiveSession }) {
  const p = s.pipeline;
  const h = s.health;
  const rows: [string, string][] = [];
  if (p) {
    rows.push(["Stream", `${streamLabel(p.videoCodec, p.videoEncoder, p.videoHardware)}, ${p.width}×${p.height}`]);
    if (p.fallbacks.length) rows.push(["Fell back from", fallbacksLabel(p.fallbacks)]);
    rows.push(
      ["Rendering", renderingLabel(p.renderer, p.rendererSoftware)],
      ["Audio", audioLabel(p.audioCodec, p.audioKbps, p.audioSourceHz)],
      ["Run-ahead", runAheadLabel(p.runAhead)],
    );
  }
  if (h) {
    rows.push(
      ["Frames late", `${Math.round(h.overran * 100)}% (mean frame ${h.tickMeanMs.toFixed(1)} ms)`],
      ["Frame work", frameWorkLabel(h)],
      [
        "Bitrate",
        `${Math.round(h.videoKbps).toLocaleString("en-US")} of ${h.videoKbpsTarget.toLocaleString("en-US")} kbps target${
          h.estimateKbps != null ? ` · bandwidth ~${Math.round(h.estimateKbps).toLocaleString("en-US")} kbps` : ""
        }`,
      ],
    );
  }
  const c = s.clientStats;
  if (c) {
    const ms = (x: number | null | undefined) => (x == null ? "–" : `${x.toFixed(1)} ms`);
    if (c.inputToFrameMs != null)
      rows.push(["Button→frame", `${ms(c.inputToFrameMs)} measured on the player's device`]);
    const decoder = decoderSummary(s);
    if (decoder) rows.push(["Player's decoder", decoder]);
    rows.push([
      "Player's view",
      `input round trip ${ms(c.inputRttMs)} · network ${ms(c.netRttMs)} · decode ${ms(c.decodeMs)}`,
    ]);
  }
  rows.push(["Core", s.core]);
  return (
    <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-6 gap-y-1 text-sm">
      {rows.map(([label, value]) => (
        <Fragment key={label}>
          <dt className="text-slate-400">{label}</dt>
          <dd className="break-words text-slate-200">{value}</dd>
        </Fragment>
      ))}
    </dl>
  );
}

/** One failed game: what happened, and the core's last words folded under it. */
function FailureItem({ f }: { f: SessionFailure }) {
  const when = new Date(f.at * 1000).toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
  return (
    <li className="py-3">
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <a
          className="font-semibold hover:underline"
          href={`/system/?id=${encodeURIComponent(f.system)}&game=${f.gameId}`}
        >
          {f.title}
        </a>
        <span className="text-amber-300">{failureLabel(f)}</span>
        <span className="text-xs text-slate-400">
          {[
            f.system.toUpperCase(),
            f.core,
            f.username ?? "(deleted account)",
            when,
            f.reason === "refused" ? null : `ran ${durationLabel(f.ranSeconds)}`,
          ]
            .filter(Boolean)
            .join(" · ")}
        </span>
        <a
          className="ml-auto text-xs text-slate-400 hover:text-white hover:underline"
          href={`/admin/settings/?id=${encodeURIComponent(f.system)}`}
        >
          Emulator settings
        </a>
      </div>
      {f.message && <p className="mt-1 text-slate-300">{f.message}</p>}
      {f.lastLines.length > 0 && (
        <details className="mt-2">
          <summary className="cursor-pointer text-xs text-slate-400 hover:text-white">
            Last {f.lastLines.length === 1 ? "line" : `${f.lastLines.length} lines`} from the emulator
          </summary>
          <pre className="mt-2 max-h-64 overflow-auto rounded-lg bg-black/40 p-3 text-xs whitespace-pre-wrap text-slate-300">
            {f.lastLines.join("\n")}
          </pre>
        </details>
      )}
    </li>
  );
}

function Failures() {
  const failures = useQuery({ ...q.sessionFailures(), refetchInterval: 10_000 });
  if (!failures.data) return null;
  return (
    <section className={panel} aria-labelledby="failures-heading">
      <h2 id="failures-heading" className="flex items-center gap-2 text-xl font-bold">
        <TriangleAlert className="size-5 text-slate-400" /> Recent failures
      </h2>
      <p className="mt-2 text-sm text-slate-400">
        Games that would not start, or crashed while playing, newest first, with the emulator's last words: a missing
        BIOS or an option it cannot run with shows here. Kept until the server restarts.
      </p>
      {failures.data.length === 0 ? (
        <p className="mt-6 text-sm text-slate-400">No game has failed since the server started.</p>
      ) : (
        <ul className="mt-2 divide-y divide-white/10 text-sm">
          {failures.data.map((f) => (
            <FailureItem key={`${f.at}-${f.gameId}`} f={f} />
          ))}
        </ul>
      )}
    </section>
  );
}

function Sessions() {
  const sessions = useQuery({ ...q.activeSessions(), refetchInterval: 2000 });
  const viewer = useQuery(q.viewer());
  const [open, setOpen] = useState<string | null>(null);
  // The session the admin asked to stop, waiting for their confirmation.
  const [stopping, setStopping] = useState<{ id: string; player: string; title: string } | null>(null);
  const stop = useMutation({
    mutationFn: api.stopSession,
    onSettled: () => invalidate(["admin", "sessions"]),
  });
  useEffect(() => {
    if (viewer.data && !viewer.data.user?.isAdmin) go("/", { replace: true });
  }, [viewer.data]);

  if (!sessions.data) return null;
  const now = Date.now() / 1000;
  return (
    <main className="mx-auto grid max-w-6xl gap-6 px-6 pt-16 pb-16">
      <section className={panel}>
        <h2 className="flex items-center gap-2 text-xl font-bold">
          <MonitorPlay className="size-5 text-slate-400" /> Active sessions
        </h2>
        <p className="mt-2 text-sm text-slate-400">
          Games being played now, and how each stream is made. The player's own numbers show while their stats panel is
          open.
        </p>
        {sessions.data.length === 0 ? (
          <p className="mt-6 text-sm text-slate-400">No games are being played.</p>
        ) : (
          <div className="mt-4 overflow-x-auto">
            <table className="w-full text-left text-sm">
              <thead className="text-xs tracking-wide text-slate-400 uppercase">
                <tr>
                  <th className="w-6 py-2" aria-label="Details" />
                  <th className="py-2 pr-4 font-semibold">Player</th>
                  <th className="py-2 pr-4 font-semibold">Game</th>
                  <th className="py-2 pr-4 font-semibold">Stream</th>
                  <th className="hidden py-2 pr-4 font-semibold md:table-cell">Rendering</th>
                  <th className="hidden py-2 pr-4 font-semibold lg:table-cell">Live</th>
                  <th className="py-2 pr-4 font-semibold">Started</th>
                  <th className="py-2" aria-label="Stop" />
                </tr>
              </thead>
              <tbody className="divide-y divide-white/10">
                {sessions.data.map((s) => {
                  const health = healthSummary(s);
                  const client = clientSummary(s);
                  const expanded = open === s.id;
                  return (
                    <Fragment key={s.id}>
                      <tr className="align-top">
                        <td className="py-3">
                          <button
                            type="button"
                            className="rounded text-slate-400 hover:text-white focus-visible:outline-2"
                            aria-expanded={expanded}
                            aria-label={`${expanded ? "Hide" : "Show"} details for ${s.title}`}
                            onClick={() => setOpen(expanded ? null : s.id)}
                          >
                            {expanded ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />}
                          </button>
                        </td>
                        <td className="py-3 pr-4">
                          <div className="font-semibold">{s.username ?? "(deleted account)"}</div>
                          <div className="text-xs text-slate-400">{s.client ?? "client not named"}</div>
                        </td>
                        <td className="py-3 pr-4">
                          <a
                            className="font-semibold hover:underline"
                            href={`/system/?id=${encodeURIComponent(s.system)}&game=${s.gameId}`}
                          >
                            {s.title}
                          </a>
                          <div className="text-xs text-slate-400">
                            {s.system.toUpperCase()} ·{" "}
                            {s.connected ? (
                              <span className="text-emerald-300">streaming</span>
                            ) : (
                              <span className="text-amber-300">waiting for the player</span>
                            )}
                          </div>
                        </td>
                        <td className="py-3 pr-4">
                          {s.pipeline ? (
                            <StreamPill p={s.pipeline} />
                          ) : (
                            <span className="text-slate-400">not started</span>
                          )}
                        </td>
                        <td className="hidden py-3 pr-4 md:table-cell">
                          {s.pipeline ? <RenderPill p={s.pipeline} /> : <span className="text-slate-400">–</span>}
                        </td>
                        <td className="hidden py-3 pr-4 lg:table-cell">
                          {health ? (
                            <div className={health.struggling ? "text-amber-300" : "text-slate-200"}>{health.text}</div>
                          ) : (
                            <span className="text-slate-400">–</span>
                          )}
                          {client && <div className="text-xs text-slate-400">Player: {client}</div>}
                        </td>
                        <td className="py-3 whitespace-nowrap">
                          <div>{new Date(s.startedAt * 1000).toLocaleTimeString([], { timeStyle: "short" })}</div>
                          <div className="text-xs text-slate-400">{durationLabel(now - s.startedAt)}</div>
                        </td>
                        <td className="py-3">
                          <Button
                            variant="outline"
                            aria-label={`Stop ${s.title} for ${s.username ?? "this player"}`}
                            disabled={stop.isPending && stop.variables === s.id}
                            onClick={() =>
                              setStopping({ id: s.id, player: s.username ?? "this player", title: s.title })
                            }
                          >
                            <Square /> Stop
                          </Button>
                        </td>
                      </tr>
                      {expanded && (
                        <tr>
                          <td />
                          <td colSpan={7} className="pb-4">
                            <Details s={s} />
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        {stop.error && <p className="mt-3 text-sm text-amber-300">{stop.error.message}</p>}
      </section>
      <Failures />
      <Confirm
        open={stopping !== null}
        onOpenChange={(open) => !open && setStopping(null)}
        title={stopping ? `Stop ${stopping.title} for ${stopping.player}?` : ""}
        description="Their progress is saved first, as when they quit: the battery save, and an auto save they can load next time."
        action="Stop the game"
        onAction={() => {
          if (stopping) stop.mutate(stopping.id);
          setStopping(null);
        }}
      />
    </main>
  );
}

export default function SessionsAdmin() {
  return (
    <Shell>
      <SideNav section="admin" />
      <div className={NAV_SPACE}>
        <Sessions />
      </div>
    </Shell>
  );
}
