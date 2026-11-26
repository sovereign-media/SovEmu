// Users: the accounts on this server and what each may do. An admin adds accounts, makes them
// admins or viewers, sets their passwords, signs them out everywhere and removes them. The server
// keeps one admin at least, and refuses changes to accounts the environment declares
// (`SovEmu__Authentication__AdminUsername`, `SovEmu__Users__<n>__…`), which it would undo at the next start:
// those show locked, and are changed there.

import { useMutation, useQuery } from "@tanstack/react-query";
import { KeyRound, Lock, LogOut, MoreHorizontal, Shield, Trash2, User as UserIcon, UserPlus } from "lucide-react";
import { type FormEvent, type ReactNode, useState } from "react";
import { type Account, api, type Role } from "../../lib/api";
import { invalidate, q } from "../../lib/data";
import { initials } from "../../lib/systems";
import { Button } from "../ui/button";
import {
  Confirm,
  Dialog,
  Field,
  inputClass,
  Menu,
  MenuContent,
  MenuItem,
  MenuSeparator,
  MenuTrigger,
  Pill,
  Segmented,
  Tooltip,
} from "../ui/primitives";
import { useToast } from "../ui/toast";
import AdminPage, { AdminSection, ErrorNote } from "./AdminPage";
import UserDetails from "./UserDetails";

const ROLES: { value: Role; label: string }[] = [
  { value: "VIEWER", label: "Viewer" },
  { value: "ADMIN", label: "Admin" },
];

type RoleBadgeProps = { role: Role };

function RoleBadge({ role }: RoleBadgeProps) {
  const admin = role === "ADMIN";
  return (
    <Pill tone={admin ? "accent" : "neutral"}>
      {admin ? <Shield aria-hidden /> : <UserIcon aria-hidden />}
      {admin ? "Admin" : "Viewer"}
    </Pill>
  );
}

type FormDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  action: string;
  busy: boolean;
  error: Error | null;
  onSubmit: (data: FormData) => void;
  children: ReactNode;
};

/** A form in a dialog: its fields, an error, and the action that submits it. */
function FormDialog({ open, onOpenChange, title, action, busy, error, onSubmit, children }: FormDialogProps) {
  const submit = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    onSubmit(new FormData(e.currentTarget));
  };
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={title}>
      <form onSubmit={submit} className="grid gap-3 px-3 pb-3">
        {children}
        <ErrorNote error={error} />
        <div className="flex justify-end gap-2">
          <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="submit" disabled={busy}>
            {action}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function AddUser() {
  const [open, setOpen] = useState(false);
  const [role, setRole] = useState<Role>("VIEWER");
  const toast = useToast();
  const add = useMutation({
    mutationFn: (data: FormData) => api.addUser(String(data.get("username")), String(data.get("password")), role),
    onSuccess: (_, data) => {
      toast(`Added ${data.get("username")}`);
      setOpen(false);
      void invalidate(["users"]);
    },
  });
  return (
    <>
      <Button onClick={() => (add.reset(), setOpen(true))}>
        <UserPlus /> Add a user
      </Button>
      <FormDialog
        open={open}
        onOpenChange={setOpen}
        title="Add a user"
        action="Add"
        busy={add.isPending}
        error={add.error}
        onSubmit={(data) => add.mutate(data)}
      >
        <Field label="Username">
          <input name="username" required autoComplete="off" className={inputClass} />
        </Field>
        <Field label="Password (at least 8 characters)">
          <input
            name="password"
            type="password"
            required
            minLength={8}
            autoComplete="new-password"
            className={inputClass}
          />
        </Field>
        <Segmented label="Role" value={role} options={ROLES} onChange={setRole} />
      </FormDialog>
    </>
  );
}

/** What an admin is about to do to an account, waiting for their confirmation or input. */
type Pending = { kind: "password" | "sign-out" | "remove"; user: Account } | null;

function UsersList() {
  const viewer = useQuery(q.viewer());
  const me = viewer.data?.user ?? null;
  const listed = useQuery(q.users());
  const [pending, setPending] = useState<Pending>(null);
  const toast = useToast();
  // Every change goes through one mutation: one error, one refresh.
  const change = useMutation({
    mutationFn: ({ run }: { run: () => Promise<unknown>; done: string }) => run(),
    onSuccess: (_, { done }) => {
      toast(done);
      setPending(null);
    },
    onSettled: () => invalidate(["users"]),
  });
  const run = (done: string, op: () => Promise<unknown>) => change.mutate({ run: op, done });
  const close = (open: boolean) => !open && setPending(null);

  return (
    <>
      <ErrorNote error={listed.error ?? (pending ? null : change.error)} />
      {listed.data && (
        <AdminSection
          title={`${listed.data.length} ${listed.data.length === 1 ? "account" : "accounts"}`}
          actions={<AddUser />}
        >
          <ul className="mt-3 divide-y divide-white/5">
            {listed.data.map((u) => {
              const mine = me?.id === u.id;
              const other: Role = u.role === "ADMIN" ? "VIEWER" : "ADMIN";
              return (
                <li key={u.id} className="flex items-center gap-3 py-3">
                  <span
                    aria-hidden
                    className="grid size-9 shrink-0 place-items-center rounded-full bg-accent text-xs font-bold text-white"
                  >
                    {initials(u.username)}
                  </span>
                  <span className="min-w-0 flex-1 truncate font-medium">
                    <a className="hover:underline" href={`/admin/users/?id=${u.id}`}>
                      {u.username}
                    </a>
                    {mine && <span className="ml-2 text-xs text-muted">(you)</span>}
                  </span>
                  {u.declared && (
                    <Tooltip content="Declared in the environment, which sets it at every start: change it there">
                      <Pill tone="muted">
                        <Lock aria-hidden /> Environment
                      </Pill>
                    </Tooltip>
                  )}
                  <RoleBadge role={u.role} />
                  <Menu>
                    <MenuTrigger asChild>
                      <Button variant="ghost" size="icon" aria-label={`Actions for ${u.username}`}>
                        <MoreHorizontal />
                      </Button>
                    </MenuTrigger>
                    <MenuContent>
                      <MenuItem
                        disabled={u.declared}
                        onSelect={() =>
                          run(`${u.username} is now ${other === "ADMIN" ? "an admin" : "a viewer"}`, () =>
                            api.setUserRole(u.id, other),
                          )
                        }
                      >
                        {other === "ADMIN" ? <Shield /> : <UserIcon />}
                        {other === "ADMIN" ? "Make an admin" : "Make a viewer"}
                      </MenuItem>
                      <MenuItem
                        disabled={u.declared}
                        onSelect={() => (change.reset(), setPending({ kind: "password", user: u }))}
                      >
                        <KeyRound /> Set a password…
                      </MenuItem>
                      <MenuItem onSelect={() => setPending({ kind: "sign-out", user: u })}>
                        <LogOut /> Sign out everywhere…
                      </MenuItem>
                      <MenuSeparator />
                      <MenuItem disabled={u.declared || mine} onSelect={() => setPending({ kind: "remove", user: u })}>
                        <Trash2 /> Remove…
                      </MenuItem>
                    </MenuContent>
                  </Menu>
                </li>
              );
            })}
          </ul>
        </AdminSection>
      )}

      <FormDialog
        open={pending?.kind === "password"}
        onOpenChange={close}
        title={`Set ${pending?.user.username ?? ""}'s password`}
        action="Set the password"
        busy={change.isPending}
        error={change.error}
        onSubmit={(data) => {
          const user = pending?.user;
          if (user)
            run(`${user.username}'s password is set`, () => api.setUserPassword(user.id, String(data.get("password"))));
        }}
      >
        <p className="text-sm text-slate-400">They are signed out everywhere, and sign in again with it.</p>
        <Field label="New password (at least 8 characters)">
          <input
            name="password"
            type="password"
            required
            minLength={8}
            autoComplete="new-password"
            className={inputClass}
          />
        </Field>
      </FormDialog>
      <Confirm
        open={pending?.kind === "sign-out"}
        onOpenChange={close}
        title={`Sign ${pending?.user.username ?? ""} out everywhere?`}
        description="Every browser and app signed in as them is signed out, and has to sign in again."
        action="Sign out everywhere"
        onAction={() => {
          const user = pending?.user;
          if (user) run(`${user.username} is signed out everywhere`, () => api.signOutEverywhere(user.id));
        }}
      />
      <Confirm
        open={pending?.kind === "remove"}
        onOpenChange={close}
        title={`Remove ${pending?.user.username ?? ""}?`}
        description="Their account, favourites, lists, cheats and settings go, and every sign-in of theirs ends. Their saves stay on disk."
        action="Remove"
        onAction={() => {
          const user = pending?.user;
          if (user) run(`Removed ${user.username}`, () => api.removeUser(user.id));
        }}
      />
    </>
  );
}

export default function UsersAdmin() {
  // One account's page when the address names one (`?id=`), else them all.
  const id = Number(new URLSearchParams(location.search).get("id")) || null;
  return (
    <AdminPage width="max-w-3xl" description="Everyone with an account here. Admins also run the server; viewers play.">
      {id ? <UserDetails id={id} /> : <UsersList />}
    </AdminPage>
  );
}
