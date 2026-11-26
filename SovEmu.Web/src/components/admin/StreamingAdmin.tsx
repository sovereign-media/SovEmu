// Streaming: a connection test from this browser to a game port (whether it is reachable, the
// round trip, and whether it went direct or through STUN or a TURN relay), what the startup
// encoder test found on this host, then the settings that shape every game's stream, as the
// Configuration page shows them. Read-only: the settings are the environment's.

import { useMutation, useQuery } from "@tanstack/react-query";
import type { ResultOf } from "gql.tada";
import { CircleCheck, CircleX, Film, Gauge, LoaderCircle } from "lucide-react";
import { api } from "../../lib/api";
import { type ConnectionResult, PATH_LABELS, runConnectionTest } from "../../lib/connectionTest";
import { q, queryClient } from "../../lib/data";
import type { StreamingQuery } from "../../lib/queries";
import { Button } from "../ui/button";
import AdminPage, { AdminSection, ErrorNote } from "./AdminPage";
import SettingRow from "./SettingRow";

const ms = (x: number | null) => (x == null || !Number.isFinite(x) ? "–" : `${Math.round(x)} ms`);

type ResultProps = { result: ConnectionResult };

function Result({ result }: ResultProps) {
  if (!result.ok) {
    return (
      <p role="alert" className="mt-4 flex items-start gap-2 text-sm text-amber-200">
        <CircleX aria-hidden className="mt-0.5 size-4 shrink-0 text-amber-400" /> {result.reason}
      </p>
    );
  }
  const r = result.report;
  const rows: [string, string][] = [
    ["Path", `${PATH_LABELS[r.path]} (${r.protocol.toUpperCase()})`],
    ["Game port reached", r.remote ?? "–"],
    ["Round trip", `${ms(r.rttMs)} typical, ${ms(r.rttMinMs)} at best`],
    ["Measured by WebRTC", ms(r.iceRttMs)],
    ["Pings lost", `${r.lost} of ${r.sent}`],
  ];
  return (
    <div className="mt-4">
      <p className="flex items-center gap-2 text-sm font-semibold text-emerald-300">
        <CircleCheck aria-hidden className="size-4" /> This browser reaches the game streams.
      </p>
      <dl className="mt-3 grid grid-cols-[max-content_minmax(0,1fr)] gap-x-6 gap-y-1 text-sm">
        {rows.map(([label, value]) => (
          <div key={label} className="contents">
            <dt className="text-slate-400">{label}</dt>
            <dd className="text-slate-200">{value}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

type ConnectionTestProps = { seconds: number };

function ConnectionTest({ seconds }: ConnectionTestProps) {
  const test = useMutation({
    mutationFn: async () =>
      runConnectionTest({ iceServers: await queryClient.fetchQuery(q.iceServers()), answer: api.testConnection }),
  });
  return (
    <AdminSection
      icon={<Gauge />}
      title="Connection test"
      description={`Connects this browser to a free game port the way a game does, and times a few round trips. The port is held for up to ${seconds} s.`}
      actions={
        <Button onClick={() => test.mutate()} disabled={test.isPending}>
          {test.isPending ? <LoaderCircle className="animate-spin" /> : <Gauge />} Test this browser
        </Button>
      }
    >
      <div className="mt-3">
        <ErrorNote error={test.error} />
      </div>
      {test.data && <Result result={test.data} />}
    </AdminSection>
  );
}

type EncoderTest = ResultOf<typeof StreamingQuery>["streaming"]["encoderTest"];

const CODEC_NAMES: Record<string, string> = { vp8: "VP8", h264: "H.264", vp9: "VP9", av1: "AV1", h265: "H.265" };

type EncodersProps = { test: EncoderTest };

/** What the startup encoder test found: each encoder as a session runs it, and a frame's cost. */
function Encoders({ test }: EncodersProps) {
  // The sizes come from the encoders that finished; one that failed has no timings of its own.
  const sizes = [...new Set(test.encoders.flatMap((e) => e.timings.map((t) => `${t.width}×${t.height}`)))];
  const seconds = (test.tookMs / 1000).toFixed(1);
  return (
    <AdminSection
      icon={<Film />}
      title="Encoders"
      description={`Every video encoder, run on a moving picture as a game would run it and timed, once, before the server takes a request. This start's test took ${seconds} s of the ${test.budgetMs / 1000} s it may.`}
    >
      {test.note && (
        <p role="alert" className="mt-3 flex items-start gap-2 text-sm text-amber-200">
          <CircleX aria-hidden className="mt-0.5 size-4 shrink-0 text-amber-400" /> The test gave no answer: {test.note}
        </p>
      )}
      {test.encoders.length > 0 && (
        <div className="mt-3 overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead className="text-slate-400">
              <tr>
                <th className="py-2 pr-4 font-semibold">Encoder</th>
                <th className="py-2 pr-4 font-semibold">Runs on</th>
                {sizes.map((size) => (
                  <th key={size} className="py-2 pr-4 text-right font-semibold">
                    {size}
                  </th>
                ))}
                <th className="py-2 font-semibold">Result</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/5">
              {test.encoders.map((e) => (
                <tr key={`${e.codec}-${e.encoder}`}>
                  <td className="py-2 pr-4 text-slate-200">
                    {CODEC_NAMES[e.codec] ?? e.codec} <span className="text-slate-400">({e.encoder})</span>
                  </td>
                  <td className="py-2 pr-4 text-slate-300">
                    {e.threads == null ? "The GPU" : `${e.threads} CPU threads`}
                  </td>
                  {sizes.map((size) => {
                    const t = e.timings.find((t) => `${t.width}×${t.height}` === size);
                    return (
                      <td
                        key={size}
                        className="py-2 pr-4 text-right tabular-nums text-slate-200"
                        title={t ? `${t.p95Ms.toFixed(2)} ms at the 95th percentile` : undefined}
                      >
                        {t ? `${t.meanMs.toFixed(2)} ms` : "–"}
                      </td>
                    );
                  })}
                  <td className="py-2">
                    {e.works ? (
                      <span className="flex items-center gap-1.5 text-emerald-300">
                        <CircleCheck aria-hidden className="size-4" /> Works
                      </span>
                    ) : (
                      <span className="flex items-start gap-1.5 text-amber-200">
                        <CircleX aria-hidden className="mt-0.5 size-4 shrink-0 text-amber-400" />{" "}
                        {e.error ?? "Does not work"}
                      </span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="mt-2 text-xs text-slate-400">
            Milliseconds a frame, on average: colour conversion, any upload to the GPU, and the encode.
          </p>
        </div>
      )}
    </AdminSection>
  );
}

function Streaming() {
  const streaming = useQuery(q.streaming());
  const s = streaming.data;
  return (
    <>
      <ErrorNote error={streaming.error} />
      {s && (
        <>
          <ConnectionTest seconds={s.testSeconds} />
          <Encoders test={s.encoderTest} />
          <AdminSection title="Settings">
            <ul className="mt-1 divide-y divide-white/5">
              {s.settings.map((setting) => (
                <SettingRow key={setting.name} setting={setting} />
              ))}
            </ul>
          </AdminSection>
        </>
      )}
    </>
  );
}

export default function StreamingAdmin() {
  return (
    <AdminPage>
      <Streaming />
    </AdminPage>
  );
}
