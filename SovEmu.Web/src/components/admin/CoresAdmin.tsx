// Cores: every libretro core the image ships (and any other in the cores folder), with its
// version, whether it loads, the systems it plays and its licence. Read-only: a system's core is
// chosen on Emulator settings or with `SOVEMU_SYSTEMS__<id>__CORE`.

import { useQuery } from "@tanstack/react-query";
import { ExternalLink, Search } from "lucide-react";
import { useState } from "react";
import { CORE_STATE_TEXT, type CoreState, coreState, coresSummary, filterCores } from "../../lib/cores";
import { q } from "../../lib/data";
import { cn } from "../ui/cn";
import { Pill, type PillTone, panel } from "../ui/primitives";
import AdminPage, { ErrorNote, Licence } from "./AdminPage";

const STATE_TONE: Record<CoreState, PillTone> = { loaded: "good", failed: "bad", missing: "muted" };

function Cores() {
  const cores = useQuery(q.cores());
  const [text, setText] = useState("");
  const all = cores.data?.cores ?? [];
  const shown = filterCores(all, text);

  return (
    <>
      <ErrorNote error={cores.error} />
      <section className={panel}>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <p className="font-semibold">{cores.data ? coresSummary(all, cores.data.libretroRelease) : "Reading…"}</p>
          <label className="flex items-center gap-2 rounded-lg border border-white/10 bg-black/30 px-3 py-1.5 text-sm focus-within:border-accent">
            <Search aria-hidden className="size-4 text-slate-500" />
            <input
              type="search"
              aria-label="Find a core"
              placeholder="Name, system or licence"
              value={text}
              onChange={(e) => setText(e.target.value)}
              className="w-48 bg-transparent outline-none placeholder:text-slate-600"
            />
          </label>
        </div>
        <p className="mt-3 text-sm leading-relaxed text-slate-400">
          The image's cores come from one libretro stable release, and each must load when the image is built. Three are
          licensed for non-commercial use only, which suits SovEmu: it is not sold. A system's core is chosen on{" "}
          <a href="/admin/settings/" className="underline">
            Emulator settings
          </a>{" "}
          or with <code>SOVEMU_SYSTEMS__&lt;id&gt;__CORE</code>.
        </p>
      </section>

      {cores.data && (
        <section className="rounded-2xl border border-white/10 bg-panel/80 px-6 py-2" aria-label="Cores">
          <ul className="divide-y divide-white/5">
            {shown.map((c) => {
              const state = coreState(c);
              return (
                <li key={c.name} className="grid gap-2 py-4 sm:grid-cols-[1fr_auto]">
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-baseline gap-x-2">
                      <span className="font-semibold">{c.label ?? c.name}</span>
                      {c.label && c.label !== c.name && (
                        <code className="font-mono text-xs text-slate-500">{c.name}</code>
                      )}
                      {c.version && <span className="text-xs text-slate-400">{c.version}</span>}
                    </div>
                    {c.systems.length > 0 && (
                      <p className="mt-1 text-sm text-slate-300">
                        {c.systems.map((s, i) => (
                          <span key={s.id}>
                            {i > 0 && ", "}
                            <a
                              href={`/admin/settings/?id=${encodeURIComponent(s.id)}`}
                              className={cn("hover:underline", !s.chosen && "text-slate-500")}
                              title={s.chosen ? "Its games start with this core" : "Another core is used"}
                            >
                              {s.name}
                            </a>
                          </span>
                        ))}
                      </p>
                    )}
                    {c.error && <p className="mt-1 text-sm break-words text-red-300">{c.error}</p>}
                  </div>
                  <div className="flex flex-wrap items-center gap-2 sm:justify-end">
                    <Pill tone={STATE_TONE[state]}>{CORE_STATE_TEXT[state]}</Pill>
                    {c.licence ? (
                      <Licence nonCommercial={c.nonCommercial}>{c.licence}</Licence>
                    ) : (
                      <span className="text-xs text-muted">Not shipped</span>
                    )}
                    {c.sourceUrl && (
                      <a
                        href={c.sourceUrl}
                        target="_blank"
                        rel="noreferrer"
                        aria-label={`${c.label ?? c.name} source`}
                        className="text-slate-500 hover:text-slate-200 [&_svg]:size-4"
                      >
                        <ExternalLink />
                      </a>
                    )}
                  </div>
                </li>
              );
            })}
            {shown.length === 0 && <li className="py-4 text-sm text-muted">No core matches.</li>}
          </ul>
        </section>
      )}
    </>
  );
}

export default function CoresAdmin() {
  return (
    <AdminPage>
      <Cores />
    </AdminPage>
  );
}
