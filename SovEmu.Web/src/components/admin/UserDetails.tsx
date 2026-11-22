// One account's page (`/admin/users/?id=…`): who it is and when it joined, what it has played,
// its saves on disk, and every browser and app signed in as it, each with a Sign out.

import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, Lock, UserRound } from "lucide-react";
import type { ReactNode } from "react";
import { q } from "../../lib/data";
import { formatBytes } from "../../lib/storage";
import { agoFromSqlite } from "../../lib/time";
import { Pill } from "../ui/primitives";
import { AdminSection, ErrorNote } from "./AdminPage";
import SignInsTable from "./SignInsTable";

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-slate-400">{label}</dt>
      <dd className="text-slate-200">{children}</dd>
    </>
  );
}

export default function UserDetails({ id }: { id: number }) {
  const details = useQuery({ ...q.userDetails(id), refetchInterval: 30_000 });
  const d = details.data;
  return (
    <>
      <a href="/admin/users/" className="inline-flex items-center gap-1 text-sm text-slate-400 hover:text-white">
        <ArrowLeft className="size-4" /> All users
      </a>
      <ErrorNote error={details.error} />
      {d && (
        <>
          <AdminSection
            icon={<UserRound />}
            title={d.user.username}
            actions={
              <>
                {d.user.declared && (
                  <Pill tone="muted">
                    <Lock aria-hidden /> Environment
                  </Pill>
                )}
                <Pill tone={d.user.role === "ADMIN" ? "accent" : "neutral"}>
                  {d.user.role === "ADMIN" ? "Admin" : "Viewer"}
                </Pill>
              </>
            }
          >
            <dl className="mt-3 grid grid-cols-[max-content_minmax(0,1fr)] gap-x-6 gap-y-1 text-sm">
              <Fact label="Joined">{agoFromSqlite(d.joinedAt)}</Fact>
              <Fact label="Played">
                {d.gamesPlayed === 0
                  ? "Nothing yet"
                  : `${d.gamesPlayed} ${d.gamesPlayed === 1 ? "game" : "games"}, ${d.starts} ${
                      d.starts === 1 ? "start" : "starts"
                    } · last ${agoFromSqlite(d.lastPlayedAt)}`}
              </Fact>
              <Fact label="Saves">
                {d.saveFiles === 0
                  ? "None"
                  : `${formatBytes(d.saveBytes)} in ${d.saveFiles} ${d.saveFiles === 1 ? "file" : "files"}`}
              </Fact>
            </dl>
          </AdminSection>
          <AdminSection title="Signed in" description="Every browser and app signed in as them.">
            <SignInsTable signIns={d.signIns} showUser={false} />
          </AdminSection>
        </>
      )}
    </>
  );
}
