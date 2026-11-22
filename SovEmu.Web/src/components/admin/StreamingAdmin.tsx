// Streaming: a connection test from this browser to a game port (whether it is reachable, the
// round trip, and whether it went direct or through STUN or a TURN relay), then the settings that
// shape every game's stream, as the Configuration page shows them. Read-only: the settings are
// the environment's.

import { useMutation, useQuery } from "@tanstack/react-query";
import { CircleCheck, CircleX, Gauge, LoaderCircle } from "lucide-react";
import { api } from "../../lib/api";
import { type ConnectionResult, PATH_LABELS, runConnectionTest } from "../../lib/connectionTest";
import { q, queryClient } from "../../lib/data";
import { Button } from "../ui/button";
import AdminPage, { AdminSection, ErrorNote } from "./AdminPage";
import SettingRow from "./SettingRow";

const ms = (x: number | null) => (x == null || !Number.isFinite(x) ? "–" : `${Math.round(x)} ms`);

function Result({ result }: { result: ConnectionResult }) {
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

function ConnectionTest({ seconds }: { seconds: number }) {
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

function Streaming() {
  const streaming = useQuery(q.streaming());
  const s = streaming.data;
  return (
    <>
      <ErrorNote error={streaming.error} />
      {s && (
        <>
          <ConnectionTest seconds={s.testSeconds} />
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
