// Needs attention: what the library lacks or could not place, a tab per kind (games without
// information, box art or a video, downloads that gave up, duplicates, games missing a disc,
// files no system plays), each with its count, its systems to narrow to, and its games. A game
// without information, box art or a video can be looked up again from here, and the downloads
// that gave up tried again. The kind and system are kept in the URL.

import { useMutation, useQuery } from "@tanstack/react-query";
import { CircleAlert, RefreshCw } from "lucide-react";
import { useState } from "react";
import { type AttentionKind, api } from "../../lib/api";
import { canLookUp, detailText, itemName, KINDS, kindFrom, tally } from "../../lib/attention";
import { invalidate, q } from "../../lib/data";
import { Button } from "../ui/button";
import { cn } from "../ui/cn";
import { Segmented } from "../ui/primitives";
import AdminPage, { AdminSection, ErrorNote } from "./AdminPage";

function keepInUrl(kind: AttentionKind, system: string | null) {
  const params = new URLSearchParams({ kind });
  if (system !== null) params.set("system", system);
  history.replaceState(null, "", `?${params}`);
}

function Attention() {
  const url = new URLSearchParams(location.search);
  const [kind, setKind] = useState(() => kindFrom(url.get("kind")));
  const [system, setSystem] = useState<string | null>(() => url.get("system"));
  const [lookedUp, setLookedUp] = useState<Set<number>>(new Set());
  const counts = useQuery(q.attentionCounts());
  const items = useQuery(q.gamesNeedingAttention(kind, system));
  const systems = useQuery(q.systems());
  const lookUp = useMutation({
    mutationFn: (gameId: number) => api.lookUpGame(gameId),
    onSuccess: (_, gameId) => setLookedUp((ids) => new Set(ids).add(gameId)),
    onSettled: () => invalidate(["admin", "status"]),
  });
  const retry = useMutation({
    mutationFn: () => api.retryArtworkDownloads(system),
    onSettled: () => {
      invalidate(["admin", "status"]);
      invalidate(["admin", "attention"]);
    },
  });

  const choose = (k: AttentionKind, s: string | null) => {
    setKind(k);
    setSystem(s);
    keepInUrl(k, s);
  };
  const { totals, systems: perSystem } = tally(counts.data ?? [], kind);
  const systemName = (id: string) =>
    id ? (systems.data?.find((s) => s.id === id)?.name ?? id) : "Folders no system uses";
  const about = KINDS.find((k) => k.value === kind);

  return (
    <AdminSection
      icon={<CircleAlert />}
      title="Needs attention"
      description="What the library lacks or could not place, as of the last scan and lookup."
      actions={
        <Button variant="outline" onClick={() => invalidate(["admin", "attention"])}>
          <RefreshCw /> Refresh
        </Button>
      }
    >
      <div className="mt-4 overflow-x-auto">
        <Segmented
          label="Kind"
          value={kind}
          options={KINDS.map((k) => ({ value: k.value, label: `${k.label} (${totals[k.value]})` }))}
          onChange={(k) => choose(k, null)}
        />
      </div>
      <p className="mt-3 text-sm text-slate-400">{about?.help}</p>
      {kind === "DOWNLOAD_GAVE_UP" && totals[kind] > 0 && (
        <div className="mt-3 flex flex-wrap items-center gap-3">
          <Button variant="outline" disabled={retry.isPending} onClick={() => retry.mutate()}>
            <RefreshCw /> {system === null ? "Try them all again" : `Try ${systemName(system)}'s again`}
          </Button>
          {retry.data !== undefined && (
            <p role="status" className="text-sm text-slate-400">
              {retry.data === 1 ? "1 download" : `${retry.data.toLocaleString()} downloads`} queued again.
            </p>
          )}
        </div>
      )}
      {perSystem.length > 0 && (
        <fieldset className="mt-3 flex flex-wrap gap-2">
          <legend className="sr-only">System</legend>
          {[{ system: null, count: totals[kind] }, ...perSystem].map((s) => (
            <button
              key={s.system ?? "all"}
              type="button"
              aria-pressed={system === s.system}
              onClick={() => choose(kind, s.system)}
              className={cn(
                "rounded-full border px-3 py-1 text-sm",
                system === s.system
                  ? "border-accent bg-accent text-white"
                  : "border-white/10 text-slate-300 hover:bg-white/5",
              )}
            >
              {s.system === null ? "All systems" : systemName(s.system)} · {s.count}
            </button>
          ))}
        </fieldset>
      )}
      <div className="mt-3">
        <ErrorNote error={counts.error ?? items.error ?? lookUp.error ?? retry.error} />
      </div>
      {items.data &&
        (items.data.length === 0 ? (
          <p className="mt-4 text-sm text-slate-400">Nothing here.</p>
        ) : (
          <ul aria-label="Games" className="mt-4 divide-y divide-white/5">
            {items.data.map((item) => (
              <li key={`${item.gameId ?? ""}${item.path}`} className="flex flex-wrap items-center gap-x-4 gap-y-1 py-2">
                <div className="min-w-0 flex-1">
                  <p className="truncate font-semibold">{itemName(item)}</p>
                  <p className="whitespace-pre-line text-sm text-slate-400">
                    {item.system && `${systemName(item.system)} · `}
                    {detailText(kind, item)}
                  </p>
                  <p className="truncate font-mono text-xs text-slate-500">{item.path}</p>
                </div>
                {item.gameId !== null && canLookUp(kind) && (
                  <Button
                    variant="outline"
                    disabled={lookUp.isPending || lookedUp.has(item.gameId)}
                    onClick={() => item.gameId !== null && lookUp.mutate(item.gameId)}
                  >
                    {lookedUp.has(item.gameId) ? "Looking up…" : "Look it up again"}
                  </Button>
                )}
              </li>
            ))}
          </ul>
        ))}
      {items.data && items.data.length >= 200 && (
        <p className="mt-2 text-sm text-slate-400">The first 200 are shown: narrow to a system to see others.</p>
      )}
    </AdminSection>
  );
}

export default function AttentionAdmin() {
  return (
    <AdminPage width="max-w-5xl">
      <Attention />
    </AdminPage>
  );
}
