// Patches: every soft patch (IPS, UPS, BPS) the scan found beside a game, grouped by system and
// game, with the one that applies by default and how many players chose each. Read-only: the
// default is set by naming a patch like its game, and players choose their own.

import { useQuery } from "@tanstack/react-query";
import { Star } from "lucide-react";
import { q } from "../../lib/data";
import { groupPatches, patchCount } from "../../lib/patches";
import { cn } from "../ui/cn";
import { panel } from "../ui/primitives";
import AdminPage, { AdminSection, ErrorNote } from "./AdminPage";

const players = (n: number) => `${n} ${n === 1 ? "player" : "players"}`;

function Patches() {
  const patches = useQuery(q.adminPatches());
  const systems = useQuery(q.systems());
  const names = new Map((systems.data ?? []).map((s) => [s.id, s.name]));
  const grouped = patches.data ? groupPatches(patches.data) : [];

  return (
    <>
      <ErrorNote error={patches.error} />
      <section className={panel}>
        <p className="font-semibold">{patches.data ? patchCount(patches.data) : "Reading…"}</p>
        <p className="mt-2 text-sm leading-relaxed text-slate-400">
          A patch named like its game (<code>Game (USA).bps</code> beside <code>Game (USA).sfc</code>) applies by
          default; others sit in a folder named like the game. Players can choose another patch, or the original, from
          the game's panel. A patched game keeps saves of its own, so renaming a patch changes which saves its players
          see.
        </p>
      </section>
      {patches.data && patches.data.length === 0 && (
        <p className="text-sm text-muted">No patches were found in the library.</p>
      )}
      {grouped.map((system) => (
        <AdminSection key={system.system} title={names.get(system.system) ?? system.system}>
          <ul className="mt-3 divide-y divide-white/5">
            {system.games.map((game) => (
              <li key={game.id} className="py-3">
                <div className="flex flex-wrap items-baseline justify-between gap-2">
                  <a
                    href={`/system/?id=${encodeURIComponent(game.system)}&game=${game.id}`}
                    className="font-semibold hover:underline"
                  >
                    {game.title}
                  </a>
                  {game.originalBy > 0 && (
                    <span className="text-xs text-muted">{players(game.originalBy)} on the original</span>
                  )}
                </div>
                <ul className="mt-2 grid gap-1.5">
                  {game.patches.map((p) => (
                    <li key={p.id} className="flex items-start gap-3 text-sm">
                      <span
                        className={cn(
                          "mt-0.5 rounded px-1.5 py-0.5 font-mono text-[11px] font-semibold",
                          p.format === "BPS"
                            ? "bg-sky-500/20 text-sky-200"
                            : p.format === "UPS"
                              ? "bg-violet-500/20 text-violet-200"
                              : "bg-white/10 text-slate-300",
                        )}
                      >
                        {p.format}
                      </span>
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <span>{p.name}</span>
                          {p.isDefault && (
                            <span className="inline-flex items-center gap-1 rounded-full bg-emerald-500/15 px-2 py-0.5 text-xs font-semibold text-emerald-300 [&_svg]:size-3">
                              <Star aria-hidden /> Default
                            </span>
                          )}
                        </div>
                        <code className="block truncate font-mono text-xs text-slate-500" title={p.file}>
                          {p.file}
                        </code>
                      </div>
                      {p.chosenBy > 0 && <span className="text-xs text-muted">chosen by {players(p.chosenBy)}</span>}
                    </li>
                  ))}
                </ul>
              </li>
            ))}
          </ul>
        </AdminSection>
      ))}
    </>
  );
}

export default function PatchesAdmin() {
  return (
    <AdminPage>
      <Patches />
    </AdminPage>
  );
}
