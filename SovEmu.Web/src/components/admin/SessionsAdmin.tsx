// Sessions: every game being played now, who is playing it on what, and how its stream is made
// (the render path, the codec and encoder, any encoder it fell back from and why) with the
// worker's live numbers, each with a Stop; then the last sessions that went wrong, with the
// worker's last lines. Like Sovereign's active streams; read every 2 s.

import { useMutation, useQuery } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, CircleAlert, MonitorPlay, Square, TriangleAlert } from "lucide-react";
import { Fragment, type ReactNode, useState } from "react";
import { type ActiveSession, api, type SessionFailure } from "../../lib/api";
import { invalidate, q } from "../../lib/data";
import { audioLabel, fallbacksLabel, renderingLabel, runAheadLabel, streamLabel } from "../../lib/pipeline";
import {
  clientSummary,
  decoderSummary,
  durationLabel,
  FAILURE_PHASE,
  failureSummary,
  frameWorkLabel,
  healthSummary,
} from "../../lib/sessions";
import { Button } from "../ui/button";
import { Confirm, panel, Tooltip } from "../ui/primitives";
import AdminPage, { ErrorNote } from "./AdminPage";

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

/** The chevron that shows or hides a row's details. */
function Expander({ expanded, label, onToggle }: { expanded: boolean; label: string; onToggle: () => void }) {
  return (
    <button
      type="button"
      className="rounded text-slate-400 hover:text-white focus-visible:outline-2"
      aria-expanded={expanded}
      aria-label={`${expanded ? "Hide" : "Show"} details for ${label}`}
      onClick={onToggle}
    >
      {expanded ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />}
    </button>
  );
}

/** A game's title, linked to its page, over its system and a note. */
function GameCell({
  gameId,
  title,
  system,
  children,
}: {
  gameId: number;
  title: string;
  system: string;
  children?: ReactNode;
}) {
  return (
    <>
      <a className="font-semibold hover:underline" href={`/system/?id=${encodeURIComponent(system)}&game=${gameId}`}>
        {title}
      </a>
      <div className="text-xs text-slate-400">
        {system.toUpperCase()}
        {children}
      </div>
    </>
  );
}

const playerName = (username: string | null) => username ?? "(deleted account)";

function ActiveSessions() {
  const sessions = useQuery({ ...q.activeSessions(), refetchInterval: 2000 });
  const [open, setOpen] = useState<string | null>(null);
  const [stopping, setStopping] = useState<ActiveSession | null>(null);
  const stop = useMutation({
    mutationFn: (id: string) => api.stopSession(id),
    onSettled: () => invalidate(q.activeSessions().queryKey),
  });

  if (!sessions.data) return null;
  const now = Date.now() / 1000;
  return (
    <section className={panel}>
      <h2 className="flex items-center gap-2 text-xl font-bold">
        <MonitorPlay className="size-5 text-slate-400" /> Active sessions
      </h2>
      <p className="mt-2 text-sm text-slate-400">
        Games being played now, and how each stream is made. The player's own numbers show while their stats panel is
        open.
      </p>
      <div className="mt-2">
        <ErrorNote error={stop.error} />
      </div>
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
                <th className="py-2" aria-label="Actions" />
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
                        <Expander
                          expanded={expanded}
                          label={s.title}
                          onToggle={() => setOpen(expanded ? null : s.id)}
                        />
                      </td>
                      <td className="py-3 pr-4">
                        <div className="font-semibold">{playerName(s.username)}</div>
                        <div className="text-xs text-slate-400">{s.client ?? "client not named"}</div>
                      </td>
                      <td className="py-3 pr-4">
                        <GameCell gameId={s.gameId} title={s.title} system={s.system}>
                          {" · "}
                          {s.connected ? (
                            <span className="text-emerald-300">streaming</span>
                          ) : (
                            <span className="text-amber-300">waiting for the player</span>
                          )}
                        </GameCell>
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
                      <td className="py-3 pr-4 whitespace-nowrap">
                        <div>{new Date(s.startedAt * 1000).toLocaleTimeString([], { timeStyle: "short" })}</div>
                        <div className="text-xs text-slate-400">{durationLabel(now - s.startedAt)}</div>
                      </td>
                      <td className="py-3 text-right">
                        <Button
                          variant="outline"
                          size="sm"
                          aria-label={`Stop ${s.title}`}
                          disabled={stop.isPending && stop.variables === s.id}
                          onClick={() => setStopping(s)}
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
      <Confirm
        open={stopping != null}
        onOpenChange={(open) => !open && setStopping(null)}
        title={`Stop ${stopping?.title ?? "this game"}?`}
        description={`${playerName(stopping?.username ?? null)}'s game ends now. Their progress is saved first.`}
        action="Stop the game"
        onAction={() => stopping && stop.mutate(stopping.id)}
      />
    </section>
  );
}

/** A failed session's worker output, and where to look next. */
function FailureDetails({ f }: { f: SessionFailure }) {
  return (
    <div className="grid gap-2 text-sm">
      {f.phase === "STARTING" ? (
        <p className="text-slate-400">The server refused it before starting the emulator.</p>
      ) : f.lastLines.length > 0 ? (
        <pre className="max-h-72 overflow-auto rounded-md bg-black/40 p-3 font-mono text-xs whitespace-pre-wrap text-slate-300">
          {f.lastLines.join("\n")}
        </pre>
      ) : (
        <p className="text-slate-400">The worker wrote nothing before it ended.</p>
      )}
      <p className="text-slate-400">
        {f.core ? `Core: ${f.core}. ` : ""}
        <a className="text-slate-200 hover:underline" href={`/admin/settings/?id=${encodeURIComponent(f.system)}`}>
          Emulator settings for {f.system.toUpperCase()}
        </a>
      </p>
    </div>
  );
}

function SessionFailures() {
  const failures = useQuery({ ...q.sessionFailures(), refetchInterval: 10_000 });
  const [open, setOpen] = useState<number | null>(null);
  if (!failures.data) return null;
  return (
    <section className={panel}>
      <h2 className="flex items-center gap-2 text-xl font-bold">
        <TriangleAlert className="size-5 text-slate-400" /> Failures
      </h2>
      <p className="mt-2 text-sm text-slate-400">
        Games that would not start, failed to load or crashed, newest first, with what the emulator said last. Kept
        until the server restarts.
      </p>
      {failures.data.length === 0 ? (
        <p className="mt-6 text-sm text-slate-400">No game has failed since the server started.</p>
      ) : (
        <div className="mt-4 overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead className="text-xs tracking-wide text-slate-400 uppercase">
              <tr>
                <th className="w-6 py-2" aria-label="Details" />
                <th className="py-2 pr-4 font-semibold">When</th>
                <th className="py-2 pr-4 font-semibold">Player</th>
                <th className="py-2 pr-4 font-semibold">Game</th>
                <th className="py-2 font-semibold">What went wrong</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/10">
              {failures.data.map((f, i) => {
                const expanded = open === i;
                return (
                  <Fragment key={`${f.at}-${f.gameId}-${f.username}`}>
                    <tr className="align-top">
                      <td className="py-3">
                        <Expander expanded={expanded} label={f.title} onToggle={() => setOpen(expanded ? null : i)} />
                      </td>
                      <td className="py-3 pr-4 whitespace-nowrap">
                        {new Date(f.at * 1000).toLocaleString([], { dateStyle: "short", timeStyle: "short" })}
                      </td>
                      <td className="py-3 pr-4 font-semibold">{playerName(f.username)}</td>
                      <td className="py-3 pr-4">
                        <GameCell gameId={f.gameId} title={f.title} system={f.system} />
                      </td>
                      <td className="py-3">
                        <div className="text-amber-200">{failureSummary(f)}</div>
                        <div className="text-xs text-slate-400">{FAILURE_PHASE[f.phase]}</div>
                      </td>
                    </tr>
                    {expanded && (
                      <tr>
                        <td />
                        <td colSpan={4} className="pb-4">
                          <FailureDetails f={f} />
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
    </section>
  );
}

export default function SessionsAdmin() {
  return (
    <AdminPage width="max-w-6xl">
      <ActiveSessions />
      <SessionFailures />
    </AdminPage>
  );
}
