// Client errors: what went wrong in the apps (a connection that failed or dropped, a picture
// that stalled, a screen that crashed, a controller nobody knows), as each client reported it,
// grouped by kind and by app and device, with how often and how lately. A group opens onto its
// events, each with its player and the game it happened in. Kept for SovEmu__ClientErrors__RetentionDays.

import { useQuery } from "@tanstack/react-query";
import { Bug, ChevronDown, ChevronRight } from "lucide-react";
import { Fragment, useState } from "react";
import type { ClientEventGroup } from "../../lib/api";
import { isFault, KIND_LABELS, splitClient, WINDOWS } from "../../lib/clientErrors";
import { q } from "../../lib/data";
import { ago } from "../../lib/time";
import { Pill, Segmented } from "../ui/primitives";
import AdminPage, { AdminSection, ErrorNote } from "./AdminPage";

type Days = (typeof WINDOWS)[number]["value"];

type EventsProps = { group: ClientEventGroup; days: number };

/** A group's events, read when it is opened. */
function Events({ group, days }: EventsProps) {
  const events = useQuery(q.clientEvents(group.kind, group.client, days));
  if (events.error) return <ErrorNote error={events.error} />;
  if (!events.data) return <p className="text-sm text-slate-400">Reading…</p>;
  return (
    <ul className="grid gap-2 text-sm">
      {events.data.map((e) => (
        <li key={`${e.at}-${e.username}-${e.message}`} className="grid gap-0.5">
          <span className="text-slate-200">{e.message}</span>
          <span className="text-xs text-slate-400">
            {new Date(e.at * 1000).toLocaleString([], { dateStyle: "medium", timeStyle: "short" })} ·{" "}
            {e.username ?? "(deleted account)"}
            {e.gameId != null && e.system ? (
              <>
                {" · in "}
                <a
                  className="text-slate-200 hover:underline"
                  href={`/system/?id=${encodeURIComponent(e.system)}&game=${e.gameId}`}
                >
                  {e.title}
                </a>
              </>
            ) : e.sessionId ? (
              ` · in a game session (${e.sessionId.slice(0, 8)})`
            ) : null}
          </span>
        </li>
      ))}
    </ul>
  );
}

function ClientErrors() {
  const [days, setDays] = useState<Days>("7");
  const [open, setOpen] = useState<string | null>(null);
  const groups = useQuery({ ...q.clientEventGroups(Number(days)), refetchInterval: 30_000 });
  return (
    <AdminSection
      icon={<Bug />}
      title="What the apps reported"
      description="Grouped by what went wrong and by app and device, the most frequent first."
      actions={
        <Segmented label="Window" value={days} options={[...WINDOWS]} onChange={(d) => (setDays(d), setOpen(null))} />
      }
    >
      <div className="mt-3">
        <ErrorNote error={groups.error} />
      </div>
      {groups.data &&
        (groups.data.length === 0 ? (
          <p className="mt-4 text-sm text-slate-400">No client has reported an error in this window.</p>
        ) : (
          <div className="mt-4 overflow-x-auto">
            <table className="w-full text-left text-sm">
              <thead className="text-xs tracking-wide text-slate-400 uppercase">
                <tr>
                  <th className="w-6 py-2" aria-label="Events" />
                  <th className="py-2 pr-4 font-semibold">What</th>
                  <th className="py-2 pr-4 font-semibold">App</th>
                  <th className="py-2 pr-4 text-right font-semibold">Count</th>
                  <th className="py-2 font-semibold">Latest</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/10">
                {groups.data.map((g) => {
                  const key = `${g.kind} ${g.client}`;
                  const expanded = open === key;
                  const { app, device } = splitClient(g.client);
                  return (
                    <Fragment key={key}>
                      <tr className="align-top">
                        <td className="py-3">
                          <button
                            type="button"
                            className="rounded text-slate-400 hover:text-white focus-visible:outline-2"
                            aria-expanded={expanded}
                            aria-label={`${expanded ? "Hide" : "Show"} ${KIND_LABELS[g.kind]} from ${g.client}`}
                            onClick={() => setOpen(expanded ? null : key)}
                          >
                            {expanded ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />}
                          </button>
                        </td>
                        <td className="py-3 pr-4">
                          <Pill tone={isFault(g.kind) ? "warn" : "neutral"}>{KIND_LABELS[g.kind]}</Pill>
                        </td>
                        <td className="py-3 pr-4">
                          <div className="font-semibold">{app}</div>
                          {device && <div className="text-xs text-slate-400">{device}</div>}
                        </td>
                        <td className="py-3 pr-4 text-right whitespace-nowrap">
                          <div className="font-semibold">{g.count.toLocaleString("en-US")}</div>
                          <div className="text-xs text-slate-400">
                            {g.users} {g.users === 1 ? "player" : "players"}
                          </div>
                        </td>
                        <td className="py-3">
                          <div className="break-words text-slate-200">{g.lastMessage}</div>
                          <div className="text-xs text-slate-400">{ago(g.lastAt)}</div>
                        </td>
                      </tr>
                      {expanded && (
                        <tr>
                          <td />
                          <td colSpan={4} className="pb-4">
                            <Events group={g} days={Number(days)} />
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  );
                })}
              </tbody>
            </table>
          </div>
        ))}
    </AdminSection>
  );
}

export default function ClientErrorsAdmin() {
  return (
    <AdminPage width="max-w-5xl">
      <ClientErrors />
    </AdminPage>
  );
}
