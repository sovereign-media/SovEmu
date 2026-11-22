// Decorations: the bezel sets found in the decoration folders, and the set each system's games
// are framed with, previewed with the game's window outlined. Read-only: the choice is
// `SOVEMU_DECORATION` and `SOVEMU_SYSTEMS__<id>__DECORATION`, shown as lines to copy.

import { useQuery } from "@tanstack/react-query";
import type { DecorationsReport } from "../../lib/api";
import { q } from "../../lib/data";
import { systemDecorationEnv, windowInsets } from "../../lib/decoration";
import { cn } from "../ui/cn";
import { panel } from "../ui/primitives";
import AdminPage, { EnvLine, ErrorNote } from "./AdminPage";

type SystemDecoration = DecorationsReport["systems"][number];

function Preview({ entry }: { entry: SystemDecoration }) {
  const d = entry.preview;
  if (!d) {
    return (
      <div className="grid aspect-video place-items-center rounded-lg bg-black/40 text-xs text-muted">
        {entry.set === "none" ? "No frame" : "No image in this set"}
      </div>
    );
  }
  return (
    <div
      className="relative overflow-hidden rounded-lg bg-black/60"
      style={{ aspectRatio: `${d.width} / ${d.height}` }}
    >
      <img
        src={d.url}
        alt={`${entry.system.name} decoration`}
        loading="lazy"
        decoding="async"
        className="absolute inset-0 size-full"
      />
      {/* Where the game's picture goes. */}
      <div aria-hidden className="absolute border border-dashed border-white/40 bg-white/5" style={windowInsets(d)} />
    </div>
  );
}

function SystemCard({ entry }: { entry: SystemDecoration }) {
  return (
    <li
      className={cn(
        panel,
        "grid min-w-0 grid-cols-[minmax(0,1fr)] gap-3 p-4",
        entry.system.gameCount === 0 && "opacity-60",
      )}
    >
      <Preview entry={entry} />
      <div className="flex items-center gap-2">
        <span aria-hidden className="size-2.5 shrink-0 rounded-full" style={{ background: entry.system.color }} />
        <span className="min-w-0 flex-1 truncate font-semibold" title={entry.system.name}>
          {entry.system.name}
        </span>
        <span
          className={cn(
            "rounded-full px-2 py-0.5 text-xs font-semibold",
            entry.perSystem ? "bg-accent/20 text-accent" : "bg-white/10 text-slate-300",
          )}
          title={entry.perSystem ? "Chosen for this system" : "The default set"}
        >
          {entry.set}
        </span>
      </div>
      {entry.perSystem && <EnvLine>{systemDecorationEnv(entry.system.id, entry.set)}</EnvLine>}
    </li>
  );
}

function Decorations() {
  const report = useQuery(q.decorations());
  const data = report.data;
  return (
    <>
      <ErrorNote error={report.error} />
      {data && (
        <>
          <section className={panel} aria-labelledby="sets-heading">
            <h2 id="sets-heading" className="text-lg font-bold">
              Sets
            </h2>
            <p className="mt-1 text-sm text-slate-400">
              Games are framed with <EnvLine>SOVEMU_DECORATION={data.default}</EnvLine>, or per system with{" "}
              <EnvLine>{systemDecorationEnv("<id>", "<set>")}</EnvLine> (<code>none</code> for no frame). Sets use
              Batocera's layout and are listed again after each library scan.
            </p>
            {data.sets.length === 0 ? (
              <p className="mt-3 text-sm text-muted">No sets found.</p>
            ) : (
              <table className="mt-4 w-full text-sm">
                <thead className="text-left text-xs text-muted">
                  <tr>
                    <th className="pb-2 font-medium">Set</th>
                    <th className="pb-2 font-medium">Default image</th>
                    <th className="pb-2 text-right font-medium">Systems</th>
                    <th className="pb-2 text-right font-medium">Games</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-white/5">
                  {data.sets.map((s) => (
                    <tr key={s.name}>
                      <td className="py-2">
                        <code className="font-mono">{s.name}</code>
                        {s.builtin && <span className="ml-2 text-xs text-muted">built in</span>}
                      </td>
                      <td className="py-2">{s.hasDefault ? "Yes" : "No"}</td>
                      <td className="py-2 text-right tabular-nums">{s.systems.length}</td>
                      <td className="py-2 text-right tabular-nums">{s.games.toLocaleString("en-US")}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            <details className="mt-4 text-sm">
              <summary className="cursor-pointer text-slate-400">Folders searched, first wins</summary>
              <ol className="mt-2 grid list-decimal gap-1 pl-6 font-mono text-xs text-slate-300">
                {data.folders.map((f) => (
                  <li key={f} className="break-all">
                    {f}
                  </li>
                ))}
              </ol>
              <p className="mt-2 text-xs text-muted">
                Add folders with <code>SOVEMU_DECORATIONS_DIRS</code>.
              </p>
            </details>
          </section>
          <section aria-labelledby="systems-heading">
            <h2 id="systems-heading" className="mb-3 text-lg font-bold">
              Systems
            </h2>
            <ul className="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-4">
              {data.systems.map((entry) => (
                <SystemCard key={entry.system.id} entry={entry} />
              ))}
            </ul>
          </section>
        </>
      )}
    </>
  );
}

export default function DecorationsAdmin() {
  return (
    <AdminPage width="max-w-6xl">
      <Decorations />
    </AdminPage>
  );
}
