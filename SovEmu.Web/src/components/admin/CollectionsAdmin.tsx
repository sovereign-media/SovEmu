// Shared collections: the collections an admin keeps for everyone, shown on every Home beside
// each user's own. Made, renamed and deleted here; games go in from a game's panel (its
// collections menu lists these under "Shared with everyone") and come out here. Those declared
// in the environment (SovEmu__Collections__…) are locked, and every collection can be copied out
// as environment lines.

import { useMutation, useQuery } from "@tanstack/react-query";
import { FolderPlus, Library, Lock, Pencil, Trash2, X } from "lucide-react";
import { type SubmitEvent, useState } from "react";
import { api, type Collection } from "../../lib/api";
import { invalidate, q } from "../../lib/data";
import { environmentLines } from "../../lib/sharedCollections";
import { Button } from "../ui/button";
import { cn } from "../ui/cn";
import { Confirm, Dialog, Field, inputClass, Pill } from "../ui/primitives";
import AdminPage, { AdminSection, EnvironmentLines, ErrorNote } from "./AdminPage";

/** Every screen that lists collections reads them again. */
const changed = () => invalidate(["collections"], ["home"], ["game"]);

function NewCollection() {
  const create = useMutation({
    mutationFn: (name: string) => api.createSharedCollection(name),
    onSuccess: changed,
  });
  const submit = (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault();
    const form = e.currentTarget;
    create.mutate(String(new FormData(form).get("name") ?? ""), { onSuccess: () => form.reset() });
  };
  return (
    <form onSubmit={submit} className="mt-4 flex flex-wrap items-end gap-3">
      <Field label="New collection">
        <input name="name" required maxLength={64} placeholder="Family night" className={cn(inputClass, "w-64")} />
      </Field>
      <Button type="submit" disabled={create.isPending}>
        <FolderPlus /> Create
      </Button>
      <ErrorNote error={create.error} />
    </form>
  );
}

function SharedCollection({ c }: { c: Collection }) {
  const [renaming, setRenaming] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const change = useMutation({ mutationFn: (run: () => Promise<unknown>) => run(), onSuccess: changed });
  const rename = (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault();
    const name = String(new FormData(e.currentTarget).get("name") ?? "");
    change.mutate(() => api.renameSharedCollection(c.id, name), { onSuccess: () => setRenaming(false) });
  };

  return (
    <li className="py-4">
      <div className="flex flex-wrap items-center gap-3">
        <h3 className="text-lg font-bold">{c.name}</h3>
        <span className="text-sm text-slate-400">
          {c.gameCount} {c.gameCount === 1 ? "game" : "games"}
        </span>
        {c.declared && (
          <Pill tone="accent">
            <Lock aria-hidden /> Set in the environment
          </Pill>
        )}
        {!c.declared && (
          <span className="ml-auto flex gap-2">
            <Button variant="outline" onClick={() => setRenaming(true)}>
              <Pencil /> Rename
            </Button>
            <Button variant="outline" onClick={() => setDeleting(true)}>
              <Trash2 /> Delete
            </Button>
          </span>
        )}
      </div>
      <ErrorNote error={change.error} />
      {c.games.length === 0 ? (
        <p className="mt-2 text-sm text-slate-400">
          No games yet: add them from a game's panel, under Collections, Shared with everyone.
        </p>
      ) : (
        <ul aria-label={`${c.name}'s games`} className="mt-2 grid gap-1 sm:grid-cols-2">
          {c.games.map((g) => (
            <li key={g.id} className="flex items-center gap-2 rounded-lg bg-black/20 px-3 py-1.5 text-sm">
              <span className="min-w-0 flex-1 truncate">{g.title}</span>
              <span className="text-xs text-slate-500 uppercase">{g.system}</span>
              {!c.declared && (
                <button
                  type="button"
                  aria-label={`Take ${g.title} out`}
                  title="Take it out"
                  onClick={() => change.mutate(() => api.setInSharedCollection(c.id, g.id, false))}
                  className="rounded p-1 text-slate-400 hover:bg-white/10 hover:text-white"
                >
                  <X className="size-4" />
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      <Dialog open={renaming} onOpenChange={setRenaming} title={`Rename ${c.name}`}>
        <form onSubmit={rename} className="grid gap-3 p-2">
          <Field label="Name">
            <input name="name" required maxLength={64} defaultValue={c.name} autoFocus className={inputClass} />
          </Field>
          <Button type="submit">
            <Pencil /> Rename
          </Button>
        </form>
      </Dialog>
      <Confirm
        open={deleting}
        onOpenChange={setDeleting}
        title={`Delete ${c.name}?`}
        description="It goes from everyone's Home. Its games stay in the library."
        action="Delete"
        onAction={() => change.mutate(() => api.deleteSharedCollection(c.id))}
      />
    </li>
  );
}

function Collections() {
  const all = useQuery(q.collections());
  const shared = (all.data ?? []).filter((c) => c.shared);
  return (
    <>
      <AdminSection
        icon={<Library />}
        title="Shared collections"
        description="Collections everyone sees on their Home, beside their own. Only admins change them."
      >
        <NewCollection />
        <ErrorNote error={all.error} />
        {all.data &&
          (shared.length === 0 ? (
            <p className="mt-4 text-sm text-slate-400">None yet.</p>
          ) : (
            <ul aria-label="Shared collections" className="mt-2 divide-y divide-white/5">
              {shared.map((c) => (
                <SharedCollection key={c.id} c={c} />
              ))}
            </ul>
          ))}
      </AdminSection>
      <EnvironmentLines lines={environmentLines(shared)}>
        These collections as server settings. Put them in <code>.env</code> or the Helm values to make them part of the
        configuration: they are then kept in step after each scan and locked here.
      </EnvironmentLines>
    </>
  );
}

export default function CollectionsAdmin() {
  return (
    <AdminPage width="max-w-5xl">
      <Collections />
    </AdminPage>
  );
}
