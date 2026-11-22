// Telemetry: whether the server exports OpenTelemetry traces and metrics (pushed to a collector,
// never served), where to, and how each signal's last export went. Read-only: it is turned on
// with SOVEMU_OTEL_ENDPOINT.

import { useQuery } from "@tanstack/react-query";
import { CircleAlert, CircleCheck, CircleDashed, CircleOff } from "lucide-react";
import { q } from "../../lib/data";
import { type ExportResult, exportText, intervalText } from "../../lib/telemetry";
import { cn } from "../ui/cn";
import { panel } from "../ui/primitives";
import AdminPage, { AdminSection, EnvLine, ErrorNote } from "./AdminPage";
import SettingRow from "./SettingRow";

function Signal({ name, result }: { name: string; result: ExportResult }) {
  const { tone, text } = exportText(result);
  const Icon = tone === "good" ? CircleCheck : tone === "bad" ? CircleAlert : CircleDashed;
  return (
    <li className="flex items-start gap-3 py-3">
      <Icon
        aria-hidden
        className={cn(
          "mt-0.5 size-5 shrink-0",
          tone === "good" ? "text-emerald-400" : tone === "bad" ? "text-amber-400" : "text-slate-500",
        )}
      />
      <div className="min-w-0">
        <p className="font-semibold">{name}</p>
        <p className="text-sm text-slate-300">{text}</p>
        {result.error && <p className="mt-1 text-sm break-words text-amber-300">{result.error}</p>}
      </div>
    </li>
  );
}

function Telemetry() {
  // Exports happen in the background: read again every 15 s.
  const telemetry = useQuery({ ...q.telemetry(), refetchInterval: 15_000 });
  const t = telemetry.data;
  return (
    <>
      <ErrorNote error={telemetry.error} />
      {t && (
        <>
          <section className={panel} aria-labelledby="export-heading">
            <div className="flex items-start gap-3">
              {t.enabled ? (
                <CircleCheck aria-hidden className="size-6 shrink-0 text-emerald-400" />
              ) : (
                <CircleOff aria-hidden className="size-6 shrink-0 text-slate-500" />
              )}
              <div className="min-w-0">
                <h2 id="export-heading" className="text-lg font-bold">
                  {t.enabled ? "Traces and metrics are exported" : "Nothing is exported"}
                </h2>
                {t.enabled ? (
                  <dl className="mt-3 grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1.5 text-sm">
                    <dt className="text-muted">Collector</dt>
                    <dd>
                      <code className="font-mono text-xs break-all">{t.endpoint}</code>
                    </dd>
                    <dt className="text-muted">Service</dt>
                    <dd>{t.serviceName}</dd>
                    <dt className="text-muted">Metrics</dt>
                    <dd>{intervalText(t.metricIntervalMs)}</dd>
                    <dt className="text-muted">Headers</dt>
                    <dd>
                      {t.headerNames.length ? t.headerNames.join(", ") : <span className="text-muted">none</span>}
                    </dd>
                  </dl>
                ) : (
                  <p className="mt-1 text-sm text-slate-400">
                    To send traces and metrics to an OpenTelemetry collector over OTLP/HTTP, set{" "}
                    <EnvLine>SOVEMU_OTEL_ENDPOINT=http://otel-collector:4318</EnvLine> and restart. The server never
                    serves its metrics itself.
                  </p>
                )}
                <p className="mt-3 text-xs text-muted">
                  Logs go to the server's standard error as <code>{t.logFormat}</code> (<code>SOVEMU_LOG_FORMAT</code>
                  ).
                </p>
              </div>
            </div>
          </section>

          {t.enabled && (
            <AdminSection title="Exports since the server started">
              <ul className="divide-y divide-white/5">
                <Signal name="Traces" result={t.traces} />
                <Signal name="Metrics" result={t.metrics} />
              </ul>
              <p className="mt-2 text-xs text-muted">
                The first metrics are sent one interval after the server starts.
              </p>
            </AdminSection>
          )}

          <AdminSection title="Settings">
            <ul className="mt-1 divide-y divide-white/5">
              {t.settings.map((s) => (
                <SettingRow key={s.name} setting={s} />
              ))}
            </ul>
          </AdminSection>
        </>
      )}
    </>
  );
}

export default function TelemetryAdmin() {
  return (
    <AdminPage width="max-w-3xl">
      <Telemetry />
    </AdminPage>
  );
}
