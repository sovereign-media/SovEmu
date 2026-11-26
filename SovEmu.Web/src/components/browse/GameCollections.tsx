// A game's place in this user's lists, on its panel: a heart for favourites, and a menu of
// collections to put it in or take it out of, with "New collection…" (named in a dialog). An
// admin's menu also has the shared collections, but for those set in the environment.

import { useQuery } from "@tanstack/react-query";
import { Check, FolderPlus, Heart, ListPlus } from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { type SubmitEvent, useMemo, useState } from "react";
import { ApiError, api, type Game } from "../../lib/api";
import { invalidate, q, queryClient } from "../../lib/data";
import { Button } from "../ui/button";
import { cn } from "../ui/cn";
import { Dialog, Field, inputClass } from "../ui/primitives";
import { useToast } from "../ui/toast";

interface CollectionChoice {
  id: number;
  name: string;
  shared: boolean;
}

const ITEM =
  "relative flex cursor-pointer items-center gap-2 rounded-lg py-1.5 pr-3 pl-8 text-sm text-slate-200 outline-none select-none data-[highlighted]:bg-white/10 [&_svg]:size-3.5";

type ChoiceProps = {
  c: CollectionChoice;
  checked: boolean;
  onToggle: (c: CollectionChoice) => void;
};

/** One collection in the menu, ticked when the game is in it. */
function Choice({ c, checked, onToggle }: ChoiceProps) {
  return (
    <DropdownMenu.CheckboxItem
      checked={checked}
      onSelect={(e) => {
        e.preventDefault();
        onToggle(c);
      }}
      className={ITEM}
    >
      <DropdownMenu.ItemIndicator className="absolute left-2.5">
        <Check />
      </DropdownMenu.ItemIndicator>
      {c.name}
    </DropdownMenu.CheckboxItem>
  );
}

type GameCollectionsProps = { game: Game; onFavourite: (favourite: boolean) => void };

export function GameCollections({ game, onFavourite }: GameCollectionsProps) {
  const toast = useToast();
  // Its collections are asked for when its panel opens (never for a whole list of games).
  const all = useQuery(q.collections());
  const ids = useQuery(q.collectionIds(game.id));
  const isAdmin = !!useQuery(q.viewer()).data?.user?.isAdmin;
  const choices: CollectionChoice[] | null = useMemo(
    () =>
      all.data && ids.data
        ? all.data
            .filter((c) => !c.shared || (isAdmin && !c.declared))
            .map(({ id, name, shared }) => ({ id, name, shared }))
        : null,
    [all.data, ids.data, isAdmin],
  );
  const collections = choices?.filter((c) => !c.shared) ?? null;
  const shared = choices?.filter((c) => c.shared) ?? [];
  const member = useMemo(() => new Set(ids.data ?? []), [ids.data]);
  /** How many of the menu's collections hold it (not the shared ones a viewer cannot change). */
  const inMenu = choices?.filter((c) => member.has(c.id)).length ?? 0;
  const idsKey = q.collectionIds(game.id).queryKey;
  /** Shows the game in or out of collections at once; the server's answer follows. */
  const setMember = (next: Set<number>) => queryClient.setQueryData(idsKey, [...next]);
  const [naming, setNaming] = useState(false);

  const toggleFavourite = async () => {
    const next = !game.favourite;
    onFavourite(next);
    try {
      await api.setFavourite(game.id, next);
      void invalidate(["home"], ["system", game.system]);
    } catch (e) {
      onFavourite(!next);
      toast(`Could not change favourites: ${(e as Error).message}`);
    }
  };

  const toggle = async (c: CollectionChoice) => {
    const now = !member.has(c.id);
    const next = new Set(member);
    if (now) next.add(c.id);
    else next.delete(c.id);
    setMember(next);
    try {
      await (c.shared ? api.setInSharedCollection : api.setInCollection)(c.id, game.id, now);
      void invalidate(["collections"], ["home"]);
      toast(now ? `Added to ${c.name}` : `Taken out of ${c.name}`);
    } catch (e) {
      setMember(member);
      toast(`Could not change ${c.name}: ${(e as Error).message}`);
    }
  };

  const create = async (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault();
    const name = String(new FormData(e.currentTarget).get("name") ?? "");
    try {
      const made = await api.createCollection(name);
      await api.setInCollection(made.id, game.id, true);
      setMember(new Set(member).add(made.id));
      await invalidate(["collections"], ["home"]);
      setNaming(false);
      toast(`Added to ${made.name}`);
    } catch (err) {
      toast(err instanceof ApiError ? err.message : "Could not create the collection");
    }
  };

  return (
    <>
      <Button
        size="lg"
        variant="outline"
        aria-pressed={game.favourite}
        aria-label={game.favourite ? "Remove from favourites" : "Add to favourites"}
        onClick={toggleFavourite}
      >
        <Heart className={cn(game.favourite && "fill-red-400 text-red-400")} />
      </Button>
      <DropdownMenu.Root modal={false}>
        <DropdownMenu.Trigger asChild>
          <Button size="lg" variant="outline" aria-label="Collections">
            <ListPlus />
            {inMenu > 0 && <span className="text-xs tabular-nums">{inMenu}</span>}
          </Button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content
            sideOffset={8}
            align="start"
            className="z-[60] min-w-52 rounded-xl border border-edge bg-panel p-1 shadow-2xl"
          >
            <DropdownMenu.Label className="px-3 pt-1.5 pb-2 text-xs font-semibold tracking-wider text-slate-500 uppercase">
              Collections
            </DropdownMenu.Label>
            {collections === null && <p className="px-3 py-1.5 text-sm text-slate-500">Loading…</p>}
            {collections?.length === 0 && <p className="px-3 py-1.5 text-sm text-slate-500">None yet.</p>}
            {collections?.map((c) => (
              <Choice key={c.id} c={c} checked={member.has(c.id)} onToggle={toggle} />
            ))}
            {shared.length > 0 && (
              <>
                <DropdownMenu.Separator className="my-1 h-px bg-edge" />
                <DropdownMenu.Label className="px-3 pt-1.5 pb-2 text-xs font-semibold tracking-wider text-slate-500 uppercase">
                  Shared with everyone
                </DropdownMenu.Label>
                {shared.map((c) => (
                  <Choice key={c.id} c={c} checked={member.has(c.id)} onToggle={toggle} />
                ))}
              </>
            )}
            <DropdownMenu.Separator className="my-1 h-px bg-edge" />
            <DropdownMenu.Item onSelect={() => setNaming(true)} className={ITEM}>
              <FolderPlus className="absolute left-2.5" /> New collection…
            </DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      <Dialog
        open={naming}
        onOpenChange={setNaming}
        title="New collection"
        description={`It starts with ${game.title}.`}
      >
        <form onSubmit={create} className="grid gap-3 p-2">
          <Field label="Name">
            <input name="name" required maxLength={64} autoFocus placeholder="Co-op night" className={inputClass} />
          </Field>
          <Button type="submit">
            <FolderPlus /> Create
          </Button>
        </form>
      </Dialog>
    </>
  );
}
