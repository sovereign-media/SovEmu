// Rendering: which GPU 3D systems render on, every device the startup probe tried and why each
// was or wasn't used, and how each 3D system renders. Read-only: the choice is `SOVEMU_GPU`.

import { useQuery } from "@tanstack/react-query";
import { CircleAlert, CircleCheck, CircleOff } from "lucide-react";
import { q } from "../../lib/data";
import { deviceName, deviceState, probedText, type Tone, verdict } from "../../lib/rendering";
import { cn } from "../ui/cn";
import { Pill, type PillTone, panel } from "../ui/primitives";
import AdminPage, { AdminSection, EnvLine, ErrorNote } from "./AdminPage";

const TONE_TEXT: Record<Tone, string> = {
  good: "text-emerald-300",
  warn: "text-amber-300",
  off: "text-slate-400",
};

const TONE_PILL: Record<Tone, PillTone> = { good: "good", warn: "warn", off: "neutral" };

function ToneIcon({ tone }: { tone: Tone }) {
  if (tone === "good") return <CircleCheck aria-hidden className="size-6 shrink-0 text-emerald-400" />;
  if (tone === "warn") return <CircleAlert aria-hidden className="size-6 shrink-0 text-amber-400" />;
  return <CircleOff aria-hidden className="size-6 shrink-0 text-slate-500" />;
}

const RENDERING_LABEL = { GPU: "GPU", SOFTWARE: "Software", OFF: "Off" } as const;

function Rendering() {
  const gpu = useQuery(q.gpu());
  const systems = useQuery(q.systems());
  const threeD = (systems.data ?? []).filter((s) => s.needsGpu);

  return (
    <>
      <ErrorNote error={gpu.error ?? systems.error} />
      {gpu.data && (
        <>
          <section className={panel} aria-label="Verdict">
            {(() => {
              const v = verdict(gpu.data);
              return (
                <div className="flex items-start gap-3">
                  <ToneIcon tone={v.tone} />
                  <div className="min-w-0">
                    <h2 className={cn("text-lg font-bold", TONE_TEXT[v.tone])}>{v.title}</h2>
                    <p className="mt-1 text-sm break-words text-slate-300">{v.detail}</p>
                    {gpu.data.note && <p className="mt-2 text-sm text-amber-300">{gpu.data.note}</p>}
                  </div>
                </div>
              );
            })()}
            <dl className="mt-4 grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
              <dt className="text-muted">Setting</dt>
              <dd>
                <EnvLine>SOVEMU_GPU={gpu.data.setting || "auto"}</EnvLine>
                <span className="ml-2 text-xs text-muted">
                  <code>auto</code>, <code>software</code>, <code>off</code> or a render node
                </span>
              </dd>
              <dt className="text-muted">Workers get</dt>
              <dd>
                <code className="font-mono text-xs">--gpu {gpu.data.workerArg}</code>
              </dd>
            </dl>
            <p className="mt-3 text-xs text-muted">{probedText(gpu.data.probedAt)}</p>
          </section>

          <AdminSection title="Devices">
            {gpu.data.devices.length === 0 ? (
              <p className="mt-2 text-sm text-muted">
                {gpu.data.mode === "OFF" && gpu.data.probedAt == null
                  ? "3D systems are off, so nothing was probed."
                  : "The probe found no device to render on."}
              </p>
            ) : (
              <ul className="mt-3 divide-y divide-white/5">
                {gpu.data.devices.map((d) => {
                  const state = deviceState(d);
                  const facts = [d.kind, d.driver, d.vendor, d.version && `GL ${d.version}`].filter(Boolean);
                  return (
                    <li key={`${d.node ?? d.kind}-${d.renderer ?? ""}`} className="flex items-start gap-3 py-3">
                      <div className="min-w-0 flex-1">
                        <div className="font-mono text-sm">{deviceName(d)}</div>
                        <div className="text-xs text-muted">{facts.join(" · ")}</div>
                        {state.detail && (
                          <div
                            className={cn("mt-1 text-sm break-words", d.works ? "text-slate-300" : "text-amber-300")}
                          >
                            {state.detail}
                          </div>
                        )}
                      </div>
                      <Pill tone={TONE_PILL[state.tone]}>{state.label}</Pill>
                    </li>
                  );
                })}
              </ul>
            )}
          </AdminSection>
        </>
      )}

      {threeD.length > 0 && (
        <AdminSection title="3D systems">
          <ul className="mt-3 divide-y divide-white/5">
            {threeD.map((s) => (
              <li key={s.id} className="flex items-center gap-3 py-2.5">
                <a href={`/admin/settings/?id=${s.id}`} className="min-w-0 flex-1 truncate hover:underline">
                  {s.name}
                </a>
                <span className="text-xs text-muted">
                  {s.gameCount} {s.gameCount === 1 ? "game" : "games"}
                </span>
                <span className="w-20 text-right text-sm">{s.rendering ? RENDERING_LABEL[s.rendering] : "—"}</span>
              </li>
            ))}
          </ul>
        </AdminSection>
      )}
    </>
  );
}

export default function RenderingAdmin() {
  return (
    <AdminPage width="max-w-3xl">
      <Rendering />
    </AdminPage>
  );
}
