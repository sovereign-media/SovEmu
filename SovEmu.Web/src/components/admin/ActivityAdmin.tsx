// Activity: who did what, and when. Sign-ins and refusals, games started and ended, and what
// admins changed, newest first, by account and by kind, a page at a time, and exported as CSV
// (built here from the same pages: there is no download route). Job runs link to the Jobs page,
// which keeps their own history.

import { useInfiniteQuery, useMutation, useQuery } from "@tanstack/react-query";
import { Download, History } from "lucide-react";
import { useState } from "react";
import { CATEGORIES, isWarning, KIND_LABELS, toCsv } from "../../lib/activity";
import { type ActivityCategory, type ActivityEntry, type ActivityFilter, api } from "../../lib/api";
import { q } from "../../lib/data";
import { Button } from "../ui/button";
import { Pill, Segmented, Select } from "../ui/primitives";
import AdminPage, { AdminSection, ErrorNote } from "./AdminPage";

/** At most this many entries go into one CSV export. */
const EXPORT_MAX = 10_000;

/** Every entry the filter matches, page by page, up to [`EXPORT_MAX`], as a CSV download. */
async function exportCsv(filter: ActivityFilter) {
  const entries: ActivityEntry[] = [];
  let after: string | null = null;
  do {
    const page = await api.activity(filter, after, 500);
    entries.push(...page.entries);
    after = page.next;
  } while (after && entries.length < EXPORT_MAX);
  const url = URL.createObjectURL(new Blob([toCsv(entries)], { type: "text/csv" }));
  const link = Object.assign(document.createElement("a"), {
    href: url,
    download: `sovemu-activity-${new Date().toISOString().slice(0, 10)}.csv`,
  });
  link.click();
  URL.revokeObjectURL(url);
  return entries.length;
}

function Subject({ e }: { e: ActivityEntry }) {
  if (e.kind === "JOB_RUN" && e.subject) {
    return (
      <a className="hover:underline" href="/admin/jobs/">
        {e.subject}
      </a>
    );
  }
  return <>{e.subject ?? ""}</>;
}

function Activity() {
  const [userId, setUserId] = useState<number | null>(null);
  const [category, setCategory] = useState<ActivityCategory | "ALL">("ALL");
  const filter: ActivityFilter = { userId, category: category === "ALL" ? null : category };
  const users = useQuery(q.users());
  const activity = useInfiniteQuery(q.activity(filter));
  const exporting = useMutation({ mutationFn: () => exportCsv(filter) });
  const entries = activity.data?.pages.flatMap((p) => p.entries) ?? [];

  const people = [
    { value: "", label: "Everyone" },
    ...(users.data ?? []).map((u) => ({ value: String(u.id), label: u.username })),
  ];
  return (
    <AdminSection
      icon={<History />}
      title="Who did what"
      description="Sign-ins and refusals, games started and ended, and what admins changed, newest first."
      actions={
        <Button variant="outline" onClick={() => exporting.mutate()} disabled={exporting.isPending}>
          <Download /> Export CSV
        </Button>
      }
    >
      <div className="mt-4 flex flex-wrap items-center gap-3">
        <Select
          label="Account"
          value={userId == null ? "" : String(userId)}
          options={people}
          onChange={(v) => setUserId(v ? Number(v) : null)}
          className="w-56"
        />
        <Segmented label="Kind" value={category} options={CATEGORIES} onChange={setCategory} />
      </div>
      <div className="mt-3 grid gap-2">
        <ErrorNote error={activity.error ?? exporting.error} />
        {exporting.data != null && (
          <p className="text-sm text-slate-400">Exported {exporting.data.toLocaleString("en-US")} entries.</p>
        )}
      </div>
      {activity.data &&
        (entries.length === 0 ? (
          <p className="mt-4 text-sm text-slate-400">Nothing recorded for this filter yet.</p>
        ) : (
          <div className="mt-4 overflow-x-auto">
            <table className="w-full text-left text-sm">
              <thead className="text-xs tracking-wide text-slate-400 uppercase">
                <tr>
                  <th className="py-2 pr-4 font-semibold">When</th>
                  <th className="py-2 pr-4 font-semibold">Who</th>
                  <th className="py-2 pr-4 font-semibold">What</th>
                  <th className="py-2 pr-4 font-semibold">About</th>
                  <th className="hidden py-2 font-semibold md:table-cell">From</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/10">
                {entries.map((e, i) => (
                  // biome-ignore lint/suspicious/noArrayIndexKey: entries have no id of their own; the list only grows at its end
                  <tr key={i} className="align-top">
                    <td className="py-2 pr-4 whitespace-nowrap text-slate-300">
                      {new Date(e.at * 1000).toLocaleString([], { dateStyle: "short", timeStyle: "short" })}
                    </td>
                    <td className="py-2 pr-4">
                      {e.userId != null ? (
                        <a className="font-semibold hover:underline" href={`/admin/users/?id=${e.userId}`}>
                          {e.username ?? "(deleted account)"}
                        </a>
                      ) : (
                        <span className="text-slate-300">{e.username ?? "–"}</span>
                      )}
                    </td>
                    <td className="py-2 pr-4">
                      <Pill tone={isWarning(e.kind) ? "warn" : "neutral"}>{KIND_LABELS[e.kind]}</Pill>
                    </td>
                    <td className="py-2 pr-4">
                      <div className="text-slate-200">
                        <Subject e={e} />
                      </div>
                      {e.detail && <div className="text-xs text-slate-400">{e.detail}</div>}
                    </td>
                    <td className="hidden py-2 font-mono text-xs text-slate-400 md:table-cell">{e.address ?? ""}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ))}
      {activity.hasNextPage && (
        <div className="mt-4">
          <Button variant="outline" onClick={() => activity.fetchNextPage()} disabled={activity.isFetchingNextPage}>
            Load more
          </Button>
        </div>
      )}
    </AdminSection>
  );
}

export default function ActivityAdmin() {
  return (
    <AdminPage width="max-w-6xl">
      <Activity />
    </AdminPage>
  );
}
