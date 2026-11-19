// A game's place in this user's lists, on its panel: a heart for favourites, and a menu of
// collections to put it in or take it out of, with "New collection…" (named in a dialog).
import { Check, FolderPlus, Heart, ListPlus } from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { useEffect, useState, type SubmitEvent } from "react";
import { api, ApiError, type Game } from "../../lib/api";
import { Button } from "../ui/button";
import { cn } from "../ui/cn";
import { Dialog, Field, inputClass } from "../ui/primitives";
import { useToast } from "../ui/toast";

interface CollectionChoice {
  id: number;
  name: string;
}

const ITEM =
  "relative flex cursor-pointer items-center gap-2 rounded-lg py-1.5 pr-3 pl-8 text-sm text-slate-200 outline-none select-none data-[highlighted]:bg-white/10 [&_svg]:size-3.5";

export function GameCollections({ game, onFavourite }: { game: Game; onFavourite: (favourite: boolean) => void }) {
  const toast = useToast();
  const [collections, setCollections] = useState<CollectionChoice[] | null>(null);
  const [member, setMember] = useState<Set<number>>(new Set());
  const [naming, setNaming] = useState(false);

  // Its collections are asked for when its panel opens (never for a whole list of games).
  useEffect(() => {
    setCollections(null);
    let gone = false;
    Promise.all([api.collections(), api.collectionIds(game.id)])
      .then(([all, ids]) => {
        if (gone) return;
        setCollections(all.map(({ id, name }) => ({ id, name })));
        setMember(new Set(ids));
      })
      .catch(() => {});
    return () => {
      gone = true;
    };
  }, [game.id]);

  const toggleFavourite = async () => {
    const next = !game.favourite;
    onFavourite(next);
    try {
      await api.setFavourite(game.id, next);
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
      await api.setInCollection(c.id, game.id, now);
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
      setCollections((list) => [...(list ?? []), { id: made.id, name: made.name }].sort((a, b) => a.name.localeCompare(b.name)));
      setMember((m) => new Set(m).add(made.id));
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
            {member.size > 0 && <span className="text-xs tabular-nums">{member.size}</span>}
          </Button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content sideOffset={8} align="start" className="z-[60] min-w-52 rounded-xl border border-edge bg-panel p-1 shadow-2xl">
            <DropdownMenu.Label className="px-3 pt-1.5 pb-2 text-xs font-semibold tracking-wider text-slate-500 uppercase">
              Collections
            </DropdownMenu.Label>
            {collections === null && <p className="px-3 py-1.5 text-sm text-slate-500">Loading…</p>}
            {collections?.length === 0 && <p className="px-3 py-1.5 text-sm text-slate-500">None yet.</p>}
            {collections?.map((c) => (
              <DropdownMenu.CheckboxItem
                key={c.id}
                checked={member.has(c.id)}
                onSelect={(e) => {
                  e.preventDefault();
                  void toggle(c);
                }}
                className={ITEM}
              >
                <DropdownMenu.ItemIndicator className="absolute left-2.5">
                  <Check />
                </DropdownMenu.ItemIndicator>
                {c.name}
              </DropdownMenu.CheckboxItem>
            ))}
            <DropdownMenu.Separator className="my-1 h-px bg-edge" />
            <DropdownMenu.Item onSelect={() => setNaming(true)} className={ITEM}>
              <FolderPlus className="absolute left-2.5" /> New collection…
            </DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      <Dialog open={naming} onOpenChange={setNaming} title="New collection" description={`It starts with ${game.title}.`}>
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
