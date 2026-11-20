// Users: the accounts on this server and what each may do. Read-only for now; adding, changing
// and removing accounts come later (the environment can already declare them all:
// `SOVEMU_USERS__<n>__…`).
import { Shield, User as UserIcon, Users } from "lucide-react";
import { useEffect, useState } from "react";
import { adminDocumentTitle } from "../../lib/adminNav";
import { api, type User } from "../../lib/api";
import { initials } from "../../lib/systems";
import Shell from "../Shell";
import SideNav, { NAV_SPACE } from "../SideNav";
import { cn } from "../ui/cn";
import { panel } from "../ui/primitives";

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
  const [me, setMe] = useState<User | null>(null);
  const [users, setUsers] = useState<Account[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    document.title = `${adminDocumentTitle(location.pathname)} · SovEmu`;
    api.viewer().then((s) => {
      if (!s.user?.isAdmin) location.replace("/");
      setMe(s.user);
    });
    api.users().then(setUsers, (e: Error) => setError(e.message));
  }, []);

  return (
    <main className="mx-auto grid max-w-3xl gap-6 p-6">
      <header>
        <h1 className="flex items-center gap-2 text-2xl font-black tracking-tight [&_svg]:size-6 [&_svg]:text-slate-400">
          <Users aria-hidden /> Users
        </h1>
        <p className="mt-1 text-sm text-muted">
          Everyone with an account here. Admins also run the server; viewers play.
        </p>
      </header>
      {error && (
        <p role="alert" className="rounded-md bg-red-500/15 px-3 py-2 text-sm text-red-300">
          {error}
        </p>
      )}
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
    </main>
  );
}

export default function UsersAdmin() {
  return (
    <Shell>
      <SideNav section="admin" />
      <div className={NAV_SPACE}>
        <UsersList />
      </div>
    </Shell>
  );
}
