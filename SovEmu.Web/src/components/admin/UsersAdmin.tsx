// Users: the accounts on this server and what each may do. Read-only for now; adding, changing
// and removing accounts come later (the environment can already declare them all:
// `SOVEMU_USERS__<n>__…`).

import { useQuery } from "@tanstack/react-query";
import { Shield, User as UserIcon } from "lucide-react";
import type { api } from "../../lib/api";
import { q } from "../../lib/data";
import { initials } from "../../lib/systems";
import { cn } from "../ui/cn";
import { panel } from "../ui/primitives";
import AdminPage, { ErrorNote } from "./AdminPage";

type Account = Awaited<ReturnType<typeof api.users>>[number];

function RoleBadge({ role }: { role: Account["role"] }) {
  const admin = role === "ADMIN";
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-semibold [&_svg]:size-3.5",
        admin ? "bg-accent/20 text-accent" : "bg-white/10 text-slate-300",
      )}
    >
      {admin ? <Shield aria-hidden /> : <UserIcon aria-hidden />}
      {admin ? "Admin" : "Viewer"}
    </span>
  );
}

function UsersList() {
  const viewer = useQuery(q.viewer());
  const me = viewer.data?.user ?? null;
  const listed = useQuery(q.users());
  const users: Account[] | null = listed.data ?? null;

  return (
    <>
      <ErrorNote error={listed.error} />
      {users && (
        <section className={panel} aria-label="Accounts">
          <p className="mb-3 text-sm text-muted">
            {users.length} {users.length === 1 ? "account" : "accounts"}
          </p>
          <ul className="divide-y divide-white/5">
            {users.map((u) => (
              <li key={u.id} className="flex items-center gap-3 py-3">
                <span
                  aria-hidden
                  className="grid size-9 shrink-0 place-items-center rounded-full bg-accent text-xs font-bold text-white"
                >
                  {initials(u.username)}
                </span>
                <span className="min-w-0 flex-1 truncate font-medium">
                  {u.username}
                  {me?.id === u.id && <span className="ml-2 text-xs text-muted">(you)</span>}
                </span>
                <RoleBadge role={u.role} />
              </li>
            ))}
          </ul>
        </section>
      )}
    </>
  );
}

export default function UsersAdmin() {
  return (
    <AdminPage width="max-w-3xl" description="Everyone with an account here. Admins also run the server; viewers play.">
      <UsersList />
    </AdminPage>
  );
}
