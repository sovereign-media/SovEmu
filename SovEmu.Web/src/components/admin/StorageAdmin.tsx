// Storage: where the data folder's space goes (the database, each user's saves, the game cache,
// the mirror, artwork, decorations), against the filesystem's free space, with "Clear the cache".
// Measuring walks the data folder, so the server keeps a measurement a few minutes; Refresh
// measures again.

import { useMutation, useQuery } from "@tanstack/react-query";
import { RefreshCw, Trash2 } from "lucide-react";
import { useState } from "react";
import { api } from "../../lib/api";
import { q, queryClient } from "../../lib/data";
import { AREA_LABELS, formatBytes, relativePath, segments } from "../../lib/storage";
import { Button } from "../ui/button";
import { cn } from "../ui/cn";
import { Confirm, Tooltip } from "../ui/primitives";
import AdminPage, { AdminSection, ErrorNote } from "./AdminPage";

/** Segment colours: SovEmu's areas in turn, then the rest of the disk and the free space. */
const PALETTE = ["#6366f1", "#22c55e", "#f59e0b", "#06b6d4", "#ec4899", "#a855f7", "#84cc16", "#f97316"];
const OTHER = { elsewhere: "#475569", free: "rgba(255,255,255,0.08)" } as Record<string, string>;

function Storage() {
  const storage = useQuery(q.storage());
  const [confirm, setConfirm] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const refresh = useMutation({
    mutationFn: () => api.storage(true),
    onSuccess: (r) => queryClient.setQueryData(q.storage().queryKey, r),
  });
  const clear = useMutation({
    mutationFn: api.clearCache,
    onSuccess: async (freed) => {
      setMessage(`Cleared: ${formatBytes(freed)} freed.`);
      queryClient.setQueryData(q.storage().queryKey, await api.storage(true));
    },
  });

  const r = storage.data;
  const cache = r?.areas.find((a) => a.id === "cache");
  const bar = r ? segments(r) : [];
  let colour = 0;
  const colours = bar.map((s) => OTHER[s.id] ?? PALETTE[colour++ % PALETTE.length]);

  return (
    <>
      <ErrorNote error={storage.error ?? refresh.error ?? clear.error} />
      {r && (
        <>
          <AdminSection
            title={
              r.totalBytes != null && r.freeBytes != null
                ? `${formatBytes(r.freeBytes)} free of ${formatBytes(r.totalBytes)}`
                : "The data folder"
            }
            actions={
              <Button variant="outline" onClick={() => refresh.mutate()} disabled={refresh.isPending}>
                <RefreshCw className={refresh.isPending ? "animate-spin" : ""} /> Refresh
              </Button>
            }
          >
            {bar.length > 0 && (
              <>
                <div
                  role="img"
                  aria-label="How the data folder's disk is used"
                  className="mt-4 flex h-3 overflow-hidden rounded-full bg-black/40"
                >
                  {bar.map((s, i) =>
                    s.percent > 0 ? (
                      <Tooltip key={s.id} content={`${s.label}: ${formatBytes(s.bytes)}`}>
                        <span style={{ width: `${s.percent}%`, background: colours[i] }} className="h-full" />
                      </Tooltip>
                    ) : null,
                  )}
                </div>
                <ul className="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-xs text-slate-400">
                  {bar.map((s, i) => (
                    <li key={s.id} className="flex items-center gap-1.5">
                      <span aria-hidden className="size-2.5 rounded-sm" style={{ background: colours[i] }} />
                      {s.label} {formatBytes(s.bytes)}
                    </li>
                  ))}
                </ul>
              </>
            )}
            <p className="mt-3 text-xs text-muted">
              <code>{r.dataDir}</code> · measured {new Date(r.measuredAt * 1000).toLocaleTimeString("en-GB")}
            </p>
          </AdminSection>

          <AdminSection title="In the data folder">
            <table className="mt-3 w-full text-sm">
              <thead className="text-left text-xs text-muted">
                <tr>
                  <th className="pb-2 font-medium">What</th>
                  <th className="pb-2 font-medium">Where</th>
                  <th className="pb-2 text-right font-medium">Files</th>
                  <th className="pb-2 text-right font-medium">Size</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/5">
                {r.areas.map((a) => (
                  <tr key={a.id} className={cn(a.bytes === 0 && "text-slate-500")}>
                    <td className="py-2 pr-3">{AREA_LABELS[a.id] ?? a.id}</td>
                    <td className="max-w-0 truncate py-2 pr-3 font-mono text-xs text-slate-500" title={a.path}>
                      {relativePath(a.path, r.dataDir)}
                    </td>
                    <td className="py-2 text-right tabular-nums">{a.files.toLocaleString("en-US")}</td>
                    <td className="py-2 text-right tabular-nums">{formatBytes(a.bytes)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </AdminSection>

          <AdminSection title="Saves by player">
            {r.saves.length === 0 ? (
              <p className="mt-2 text-sm text-muted">No saves yet.</p>
            ) : (
              <ul className="mt-3 divide-y divide-white/5 text-sm">
                {r.saves.map((u) => (
                  <li key={u.userId} className="flex items-center gap-3 py-2">
                    <span className="flex-1">
                      {u.username ?? <span className="text-muted italic">(deleted user {u.userId})</span>}
                    </span>
                    <span className="text-xs text-muted">{u.files.toLocaleString("en-US")} files</span>
                    <span className="w-20 text-right tabular-nums">{formatBytes(u.bytes)}</span>
                  </li>
                ))}
              </ul>
            )}
          </AdminSection>

          <AdminSection title="Game cache">
            <p className="mt-2 text-sm text-slate-400">
              {r.cacheLimitBytes != null
                ? `In cache mode, games and box art are copied from the library as they are used, up to ${formatBytes(r.cacheLimitBytes)} (SOVEMU_CACHE_GB), least recently used out. `
                : "The library is not in cache mode, so nothing new is copied. "}
              It holds {formatBytes(cache?.bytes ?? 0)} now. Clearing it frees the space; games being played keep their
              copies, and the rest are copied again when next played.
            </p>
            <div className="mt-3 flex flex-wrap items-center gap-3">
              <Button
                variant="outline"
                onClick={() => setConfirm(true)}
                disabled={clear.isPending || !cache || cache.bytes === 0}
              >
                <Trash2 /> Clear the cache
              </Button>
              {message && <span className="text-sm text-emerald-300">{message}</span>}
            </div>
          </AdminSection>
        </>
      )}
      <Confirm
        open={confirm}
        onOpenChange={setConfirm}
        title="Clear the game cache?"
        description="Copies of games not being played are deleted, and copied again from the library when next played."
        action="Clear the cache"
        onAction={() => clear.mutate()}
      />
    </>
  );
}

export default function StorageAdmin() {
  return (
    <AdminPage>
      <Storage />
    </AdminPage>
  );
}
